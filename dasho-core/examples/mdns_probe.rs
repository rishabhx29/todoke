//! Debug probe: advertise + browse on one host, print every event.
use std::collections::HashMap;
use std::time::{Duration, Instant};

fn main() {
    let daemon = mdns_sd::ServiceDaemon::new().expect("daemon");
    let props = [
        ("ver", "1"),
        ("name", "Probe"),
        ("fp", "AAAABBBB-CCCCDDDD-EEEEFFFF-00001111"),
    ];
    let service = mdns_sd::ServiceInfo::new(
        "_dasho._udp.local.",
        "Probe",
        "dasho-probe.local.",
        "127.0.0.1",
        45999,
        props.as_slice(),
    )
    .expect("info");
    daemon.register(service).expect("register");
    println!("registered");

    let receiver = daemon.browse("_dasho._udp.local.").expect("browse");
    println!("browsing for 8s...");
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut seen: HashMap<String, String> = HashMap::new();
    while Instant::now() < deadline {
        match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(event) => match event {
                mdns_sd::ServiceEvent::SearchStarted(t) => println!("[ev] SearchStarted {t}"),
                mdns_sd::ServiceEvent::ServiceFound(ty, name) => {
                    println!("[ev] ServiceFound {name} in {ty}")
                }
                mdns_sd::ServiceEvent::ServiceResolved(r) => {
                    let fp = r
                        .get_property("fp")
                        .map(|p| p.val_str().to_string())
                        .unwrap_or_default();
                    println!(
                        "[ev] Resolved {} port {} fp={fp} ips={:?}",
                        r.get_fullname(),
                        r.port,
                        r.get_addresses_v4()
                    );
                    seen.insert(r.get_fullname().to_string(), fp);
                }
                mdns_sd::ServiceEvent::ServiceRemoved(_, name) => {
                    println!("[ev] ServiceRemoved {name}");
                    seen.remove(&name);
                }
                mdns_sd::ServiceEvent::SearchStopped(t) => println!("[ev] SearchStopped {t}"),
                other => println!("[ev] other {other:?}"),
            },
            Err(mdns_sd::RecvTimeoutError::Timeout) => {}
            Err(e) => println!("[err] {e}"),
        }
    }
    println!("RESULT: {seen:?}");
    let _ = daemon.stop_browse("_dasho._udp.local.");
    let _ = daemon.unregister("Probe._dasho._udp.local.");
}
