use crate::view::Settings;

#[test]
fn default_stream_listener_is_usb_loopback_only() {
    assert_eq!(Settings::default().bind_host, "127.0.0.1");
}
