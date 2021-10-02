use zbus::dbus_proxy;

#[dbus_proxy(
  interface = "com.musikid.fancy",
  default_service = "com.musikid.fancy",
  default_path = "/com/musikid/fancy"
)]
trait Fancy {
  /// SetTargetFanSpeed method
  fn set_target_fan_speed(&self, index: u8, speed: f64) -> zbus::Result<()>;

  /// Auto property
  #[dbus_proxy(property)]
  fn auto(&self) -> zbus::Result<bool>;
  #[dbus_proxy(property)]
  fn set_auto(&self, value: bool) -> zbus::Result<()>;

  /// Config property
  #[dbus_proxy(property)]
  fn config(&self) -> zbus::Result<String>;
  #[dbus_proxy(property)]
  fn set_config(&self, value: &str) -> zbus::Result<()>;

  /// Critical property
  #[dbus_proxy(property)]
  fn critical(&self) -> zbus::Result<bool>;

  /// FansNames property
  #[dbus_proxy(property)]
  fn fans_names(&self) -> zbus::Result<Vec<String>>;

  /// FansSpeeds property
  #[dbus_proxy(property)]
  fn fans_speeds(&self) -> zbus::Result<Vec<f64>>;

  /// PollInterval property
  #[dbus_proxy(property)]
  fn poll_interval(&self) -> zbus::Result<u64>;

  /// TargetFansSpeeds property
  #[dbus_proxy(property)]
  fn target_fans_speeds(&self) -> zbus::Result<Vec<f64>>;
  #[dbus_proxy(property)]
  fn set_target_fans_speeds(&self, value: &[f64]) -> zbus::Result<()>;

  /// Temperatures property
  #[dbus_proxy(property)]
  fn temperatures(&self) -> zbus::Result<std::collections::HashMap<String, f64>>;
}