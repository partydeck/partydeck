pub mod river_layout_v3 {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("./src/layout_manager/river-layout-v3.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("./src/layout_manager/river-layout-v3.xml");
}
pub mod river_control_unstable_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("./src/layout_manager/river-control-unstable-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("./src/layout_manager/river-control-unstable-v1.xml");
}
pub mod wlr_output_mgmt_unstable_v1 {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        // use wayland_client::protocol::__interfaces::*; // Unused
        wayland_scanner::generate_interfaces!(
            "./src/layout_manager/wlr-output-management-unstable-v1.xml"
        );
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!(
        "./src/layout_manager/wlr-output-management-unstable-v1.xml"
    );
}
