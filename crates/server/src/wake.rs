//! Wake Coordinator: picks Wake Relays (Online Agents on the target's subnet) from reported inventories.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use rcli_protocol::agent::Inventory;
use uuid::Uuid;

/// One way to wake the target: ask `relay` to send the magic packet to `mac`.
#[derive(Debug, PartialEq)]
pub struct Route {
    pub relay: Uuid,
    pub relay_name: String,
    pub mac: [u8; 6],
}

/// Routes through every relay sharing a subnet with one of the target's interfaces, in `relays` order,
/// or the reason why there is none.
pub fn routes(
    target_name: &str,
    target: &Inventory,
    relays: &[(Uuid, String, Inventory)],
) -> Result<Vec<Route>, String> {
    let lans: Vec<([u8; 6], IpAddr, u8)> = target
        .network_interfaces
        .iter()
        .filter_map(|i| Some((parse_mac(&i.mac)?, i)))
        .flat_map(|(mac, i)| i.ips.iter().filter_map(move |ip| net(ip).map(|(ip, prefix)| (mac, ip, prefix))))
        .collect();
    if lans.is_empty() {
        return Err(format!(
            "Device {target_name:?} has not reported a network interface with a MAC and an IP address, so no Wake Relay can be chosen"
        ));
    }
    let routes: Vec<Route> = relays
        .iter()
        .filter_map(|(relay, relay_name, inv)| {
            let relay_ips = || inv.network_interfaces.iter().flat_map(|i| &i.ips).filter_map(|ip| net(ip));
            let &(mac, ..) = lans
                .iter()
                .find(|(_, ip, prefix)| relay_ips().any(|(r, _)| masked(r, *prefix) == masked(*ip, *prefix)))?;
            Some(Route { relay: *relay, relay_name: relay_name.clone(), mac })
        })
        .collect();
    if routes.is_empty() {
        let subnets: Vec<String> = lans.iter().map(|(_, ip, p)| format!("{}/{p}", masked(*ip, *p))).collect();
        return Err(format!(
            "no Online Agent is in the LAN of Device {target_name:?} ({}); keep an always-on Device with an Agent there to act as Wake Relay",
            subnets.join(", ")
        ));
    }
    Ok(routes)
}

/// `"addr/prefix"` -> (addr, prefix); None for unparsable, loopback and link-local (they match across LANs).
fn net(s: &str) -> Option<(IpAddr, u8)> {
    let (ip, prefix) = match s.split_once('/') {
        Some((ip, p)) => (ip.parse::<IpAddr>().ok()?, p.parse().ok()?),
        // shortcut: no prefix reported = assume /24 (v4) or /64 (v6); fine for typical LANs.
        None => {
            let ip: IpAddr = s.parse().ok()?;
            (ip, if ip.is_ipv4() { 24 } else { 64 })
        }
    };
    let local = match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() || prefix > 32,
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.segments()[0] & 0xffc0 == 0xfe80 || v6.is_unspecified() || prefix > 128
        }
    };
    (!local).then_some((ip, prefix))
}

/// The network address of `ip` under `prefix` (an IPv4 address never equals an IPv6 one).
fn masked(ip: IpAddr, prefix: u8) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => {
            IpAddr::V4(Ipv4Addr::from(u32::from(v4) & u32::MAX.checked_shl(32 - prefix.min(32) as u32).unwrap_or(0)))
        }
        IpAddr::V6(v6) => IpAddr::V6(Ipv6Addr::from(
            u128::from(v6) & u128::MAX.checked_shl(128 - prefix.min(128) as u32).unwrap_or(0),
        )),
    }
}

/// "aa:bb:cc:dd:ee:ff" (or with '-'), case-insensitive; None if malformed or all zero (loopback).
fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let bytes: Vec<u8> = s.split([':', '-']).map(|b| u8::from_str_radix(b, 16).ok()).collect::<Option<_>>()?;
    let mac: [u8; 6] = bytes.try_into().ok()?;
    (mac != [0; 6]).then_some(mac)
}

pub fn format_mac(mac: [u8; 6]) -> String {
    mac.map(|b| format!("{b:02x}")).join(":")
}

#[cfg(test)]
mod tests {
    use rcli_protocol::agent::NetworkInterface;

    use super::*;

    fn inv(ifaces: &[(&str, &[&str])]) -> Inventory {
        let network_interfaces = ifaces
            .iter()
            .map(|(mac, ips)| NetworkInterface {
                name: "eth".into(),
                mac: mac.to_string(),
                ips: ips.iter().map(|s| s.to_string()).collect(),
            })
            .collect();
        Inventory { network_interfaces, ..Default::default() }
    }

    #[test]
    fn picks_relays_on_the_subnet_of_the_matching_interface() {
        let target =
            inv(&[("00:00:00:00:00:00", &["127.0.0.1/8"]), ("AA-BB-CC-00-00-01", &["10.1.2.3/16", "fe80::1/64"])]);
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let relays = vec![
            (a, "loopback-only".into(), inv(&[("00:00:00:00:00:00", &["127.0.0.1/8", "fe80::2/64"])])),
            (b, "same-lan".into(), inv(&[("02:00:00:00:00:02", &["10.1.200.9/16"])])),
            (c, "other-lan".into(), inv(&[("02:00:00:00:00:03", &["10.2.2.3/16"])])),
        ];
        let routes = routes("t", &target, &relays).unwrap();
        assert_eq!(routes, vec![Route { relay: b, relay_name: "same-lan".into(), mac: [0xaa, 0xbb, 0xcc, 0, 0, 1] }]);
    }

    #[test]
    fn ipv6_subnets_match_too() {
        let target = inv(&[("aa:bb:cc:00:00:01", &["2001:db8:1::5/64"])]);
        let relays = vec![(Uuid::nil(), "r".into(), inv(&[("02:00:00:00:00:02", &["2001:db8:1::9/64"])]))];
        assert_eq!(routes("t", &target, &relays).unwrap().len(), 1);
        let relays = vec![(Uuid::nil(), "r".into(), inv(&[("02:00:00:00:00:02", &["2001:db8:2::9/64"])]))];
        assert!(routes("t", &target, &relays).is_err());
    }

    #[test]
    fn explains_why_no_relay_can_be_chosen() {
        let none = routes("t", &inv(&[("aa:bb:cc:00:00:01", &[])]), &[]).unwrap_err();
        assert!(none.contains("has not reported a network interface"), "{none}");
        let target = inv(&[("aa:bb:cc:00:00:01", &["192.168.1.20/24"])]);
        let far = routes("t", &target, &[(Uuid::nil(), "r".into(), inv(&[("02:00:00:00:00:02", &["10.0.0.1/24"])]))]);
        assert!(far.unwrap_err().contains("192.168.1.0/24"));
    }

    #[test]
    fn formats_macs_lowercase_with_colons() {
        assert_eq!(format_mac([0xaa, 0xbb, 0xcc, 0, 0, 1]), "aa:bb:cc:00:00:01");
    }
}
