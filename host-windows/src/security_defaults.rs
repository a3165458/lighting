use crate::view::{effective_bind_host, Settings};

#[test]
fn default_stream_listener_is_usb_loopback_only() {
    assert_eq!(Settings::default().bind_host, "127.0.0.1");
    assert_eq!(effective_bind_host(""), "127.0.0.1");
    assert_eq!(effective_bind_host("  "), "127.0.0.1");
    assert_eq!(effective_bind_host("0.0.0.0"), "0.0.0.0");
}
