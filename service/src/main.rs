/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::time::Duration;

use async_std::future;
use async_std::{channel, sync::Arc, task};
use config::Config;
use futures::{try_join, StreamExt, TryFutureExt};
use loader::Loader;
use once_cell::sync::Lazy;
use signal_hook::consts::*;
use signal_hook_async_std::Signals;
use smol::Timer;
use snafu::{ResultExt, Snafu};

use ec_control::{ECManager, EcAccess, Event, ExternalEvent, RawPort, RW};
use nbfc_config as nbfc;
use temp::Temperatures;

use crate::ec_control::EcRW;

mod config;
mod constants;
mod ec_control;
mod loader;
mod state;
mod temp;

type Result<T> = std::result::Result<T, ServiceError>;

#[derive(Debug, Snafu)]
enum ServiceError {
    #[snafu(display("An error occurred while opening EC: {}", source))]
    OpenDev {
        source: async_std::io::Error,
    },

    #[snafu(display("{}", source))]
    ECIO {
        source: ec_control::EcManagerError,
    },

    #[snafu(display("{}", source))]
    Sensor {
        source: temp::SensorError,
    },

    #[snafu(display("{}", source))]
    DBus {
        source: zbus::Error,
    },

    ConfigErr {
        source: config::ConfigError,
    },

    #[snafu(display("{}", source))]
    Signal {
        source: std::io::Error,
    },

    #[snafu(display("{}", source))]
    ShutdownChannelRecv {
        source: async_std::channel::RecvError,
    },

    #[snafu(display("{}", source))]
    ShutdownChannelSend {
        source: async_std::channel::SendError<bool>,
    },
}

#[async_std::main]
async fn main() -> Result<()> {
    //TODO: Check errors
    let mut config = Config::load_config()
        .await
        .unwrap_or_else(|_| config::Config::default());

    let conn = zbus::Connection::system().await.context(DBusSnafu {})?;
    conn.request_name("com.musikid.fancy")
        .await
        .context(DBusSnafu {})?;
    let conn = Arc::from(conn);

    let mut temps = Temperatures::new(config.sensors.only.clone())
        .await
        .context(SensorSnafu {})?;

    //TODO: Check errors
    let ec_device = EcAccess::from_mode(config.core.ec_access_mode)
        .or_else(|_| EcAccess::try_default())
        .await
        .context(OpenDevSnafu {})?;
    let mode = ec_device.mode();

    let mut signals = Signals::new(&[SIGHUP, SIGTERM, SIGINT, SIGQUIT]).context(SignalSnafu)?;
    let (shutdown_tx, shutdown_rx) = channel::bounded(1);
    let sig_handle = signals.handle();
    let signal_handler = task::spawn(async move {
        while let Some(sig) = signals.next().await {
            match sig {
                //TODO: Reload configuration?
                SIGHUP => {}
                SIGTERM | SIGINT | SIGQUIT => {
                    shutdown_tx.send(true).await.context(ShutdownChannelSendSnafu)?;
                    sig_handle.close();
                    break;
                }
                _ => {}
            }
        }

        Ok::<_, ServiceError>(())
    });

    let mut manager = ECManager::new(ec_device, Arc::clone(&conn));

    let loader = Loader::new(manager.create_sender()).await;
    conn.object_server()
        .at("/com/musikid/fancy/loader", loader)
        .await
        .context(DBusSnafu)?;

    let shutdown_recv = shutdown_rx.clone();
    //TODO: Set interval?
    let ev_sender = manager.create_sender();
    let temps_task = task::spawn(async move {
        loop {
            match future::timeout(Duration::from_millis(100), shutdown_recv.recv()).await {
                Ok(res) => {
                    if res.context(ShutdownChannelRecvSnafu)? {
                        break Ok::<_, ServiceError>(());
                    }
                }
                // Loop timeout
                Err(_) => {
                    let temp = temps.get_temp().await.context(SensorSnafu {})?;
                    ev_sender
                        .send_event(Event::External(ExternalEvent::TempChange(temp)))
                        .await
                }
            }
        }
    });

    let shutdown_recv = shutdown_rx.clone();
    let ev_sender = manager.create_sender();
    let manager_task = task::spawn(async move {
        // We need to send the shutdown signal to the event loop
        task::spawn(async move {
            shutdown_recv.recv().await.context(ShutdownChannelRecvSnafu)?;
            ev_sender
                .send_event(Event::External(ExternalEvent::Shutdown))
                .await;
            Ok::<_, ServiceError>(())
        });

        manager.event_handler().await.context(ECIOSnafu)?;
        manager.target_speeds().await.context(ECIOSnafu)
    });

    signal_handler.await?;
    let target_speeds = manager_task.await?;
    temps_task.await?;

    // Save the configuration
    let loader_ref = conn
        .object_server()
        .interface::<_, Loader>("/com/musikid/fancy/loader")
        .await
        .context(DBusSnafu)?;
    let loader = loader_ref.get().await;

    if let Some(fan_config) = loader.current_config.as_ref().map(|t| t.0.clone()) {
        config.fan_config.selected_fan_configuration = fan_config;
    }
    if !target_speeds.is_empty() {
        config.fan_config.target_speeds = target_speeds;
    }
    config.core.ec_access_mode = mode;

    config.save_config().await.context(ConfigErrSnafu)?;

    Ok(())
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::fs::{read_dir, OpenOptions};
    use std::io::Read;
    use std::path::PathBuf;

    use rayon::prelude::*;
    use rstest::fixture;

    use nbfc_config::{FanControlConfigV2, XmlFanControlConfigV2};

    #[fixture]
    #[once]
    pub fn parsed_configs() -> Vec<FanControlConfigV2> {
        let paths: Vec<PathBuf> = read_dir("nbfc_configs/Configs")
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();

        paths
            .par_iter()
            .map(|path| {
                let mut file = OpenOptions::new().read(true).open(path).unwrap();

                let mut buf = String::with_capacity(4096);
                file.read_to_string(&mut buf).unwrap();
                buf
            })
            .map(|s| {
                //TODO: Other extensions
                quick_xml::de::from_str::<XmlFanControlConfigV2>(&s)
                    .unwrap()
                    .into()
            })
            .collect()
    }
}
