use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};

use anyhow::Result;

use crate::models::LocalIpAddressOption;

const LOOPBACK_FALLBACK: &str = "127.0.0.1";

#[derive(Debug, Clone, PartialEq, Eq)]
struct AddressCandidate {
    discovery_index: usize,
    interface_name: String,
    ip: Ipv4Addr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum AddressRank {
    PrivateLan = 0,
    OtherLan = 1,
    PrivateVirtual = 2,
    OtherVirtual = 3,
    Tailscale = 4,
}

pub fn preferred_local_ipv4_address() -> Result<String> {
    Ok(ordered_local_ipv4_addresses()?
        .into_iter()
        .next()
        .unwrap_or_else(|| LOOPBACK_FALLBACK.to_string()))
}

pub fn ordered_local_ipv4_addresses() -> Result<Vec<String>> {
    let addresses = ordered_local_ipv4_address_options()?
        .into_iter()
        .map(|option| option.address)
        .collect::<Vec<_>>();

    if addresses.is_empty() {
        return Ok(vec![LOOPBACK_FALLBACK.to_string()]);
    }

    Ok(addresses)
}

pub fn ordered_local_ipv4_address_options() -> Result<Vec<LocalIpAddressOption>> {
    let options = ordered_candidates(collect_candidates()?)
        .into_iter()
        .map(|candidate| LocalIpAddressOption {
            address: candidate.ip.to_string(),
            interface_name: candidate.interface_name,
        })
        .collect::<Vec<_>>();

    if options.is_empty() {
        return Ok(vec![LocalIpAddressOption {
            address: LOOPBACK_FALLBACK.to_string(),
            interface_name: "Loopback".to_string(),
        }]);
    }

    Ok(options)
}

fn collect_candidates() -> Result<Vec<AddressCandidate>> {
    Ok(if_addrs::get_if_addrs()?
        .into_iter()
        .enumerate()
        .filter_map(|(discovery_index, interface)| match interface.addr.ip() {
            IpAddr::V4(ip) if is_usable_ipv4(ip) => Some(AddressCandidate {
                discovery_index,
                interface_name: interface.name,
                ip,
            }),
            _ => None,
        })
        .collect::<Vec<_>>())
}

fn ordered_candidates(candidates: Vec<AddressCandidate>) -> Vec<AddressCandidate> {
    let mut ranked = candidates
        .into_iter()
        .map(|candidate| {
            (
                rank_ipv4_candidate(&candidate.interface_name, candidate.ip),
                candidate.discovery_index,
                candidate,
            )
        })
        .collect::<Vec<_>>();

    ranked.sort_by_key(|(rank, discovery_index, candidate)| {
        (*rank, *discovery_index, candidate.ip.octets())
    });

    let mut seen = HashSet::new();
    let mut ordered = Vec::with_capacity(ranked.len());
    for (_, _, candidate) in ranked {
        if seen.insert(candidate.ip) {
            ordered.push(candidate);
        }
    }

    ordered
}

fn rank_ipv4_candidate(interface_name: &str, ip: Ipv4Addr) -> AddressRank {
    if is_tailscale_name(interface_name) || is_tailscale_cgnat(ip) {
        return AddressRank::Tailscale;
    }

    let is_virtual = is_likely_virtual_interface(interface_name);
    let is_private = ip.is_private();

    match (is_private, is_virtual) {
        (true, false) => AddressRank::PrivateLan,
        (false, false) => AddressRank::OtherLan,
        (true, true) => AddressRank::PrivateVirtual,
        (false, true) => AddressRank::OtherVirtual,
    }
}

fn is_usable_ipv4(ip: Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified()
}

fn is_tailscale_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    normalized == "tailscale" || normalized.starts_with("tailscale")
}

fn is_tailscale_cgnat(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..128).contains(&octets[1])
}

fn is_likely_virtual_interface(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    [
        "docker",
        "vethernet",
        "hyper-v",
        "virtualbox",
        "virtual",
        "vmware",
        "vbox",
        "wsl",
        "loopback",
        "hamachi",
        "zerotier",
        "bridge",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::{
        is_likely_virtual_interface, is_tailscale_cgnat, is_tailscale_name, ordered_candidates,
        AddressCandidate,
    };

    fn candidate(discovery_index: usize, interface_name: &str, ip: [u8; 4]) -> AddressCandidate {
        AddressCandidate {
            discovery_index,
            interface_name: interface_name.to_string(),
            ip: Ipv4Addr::from(ip),
        }
    }

    #[test]
    fn prefers_private_lan_before_virtual_and_tailscale() {
        let ordered = ordered_candidates(vec![
            candidate(0, "tailscale0", [100, 100, 2, 5]),
            candidate(1, "vEthernet (WSL)", [172, 28, 64, 1]),
            candidate(2, "Wi-Fi", [192, 168, 1, 44]),
            candidate(3, "Ethernet", [10, 0, 0, 15]),
        ]);

        assert_eq!(
            ordered
                .into_iter()
                .map(|candidate| candidate.ip)
                .collect::<Vec<_>>(),
            vec![
                Ipv4Addr::new(192, 168, 1, 44),
                Ipv4Addr::new(10, 0, 0, 15),
                Ipv4Addr::new(172, 28, 64, 1),
                Ipv4Addr::new(100, 100, 2, 5),
            ]
        );
    }

    #[test]
    fn keeps_non_virtual_public_before_virtual_private() {
        let ordered = ordered_candidates(vec![
            candidate(0, "vEthernet (DockerNAT)", [172, 29, 224, 1]),
            candidate(1, "Ethernet", [198, 51, 100, 44]),
        ]);

        assert_eq!(
            ordered
                .into_iter()
                .map(|candidate| candidate.ip)
                .collect::<Vec<_>>(),
            vec![
                Ipv4Addr::new(198, 51, 100, 44),
                Ipv4Addr::new(172, 29, 224, 1),
            ]
        );
    }

    #[test]
    fn deduplicates_repeated_ips() {
        let ordered = ordered_candidates(vec![
            candidate(0, "Wi-Fi", [192, 168, 0, 10]),
            candidate(1, "Ethernet", [192, 168, 0, 10]),
        ]);

        assert_eq!(
            ordered
                .into_iter()
                .map(|candidate| candidate.ip)
                .collect::<Vec<_>>(),
            vec![Ipv4Addr::new(192, 168, 0, 10)]
        );
    }

    #[test]
    fn keeps_the_interface_name_for_the_selected_ip() {
        let ordered = ordered_candidates(vec![
            candidate(0, "tailscale0", [100, 100, 2, 5]),
            candidate(1, "Ethernet", [192, 168, 0, 10]),
        ]);

        assert_eq!(ordered[0].interface_name, "Ethernet");
        assert_eq!(ordered[0].ip, Ipv4Addr::new(192, 168, 0, 10));
    }

    #[test]
    fn detects_tailscale_by_name_and_range() {
        assert!(is_tailscale_name("tailscale0"));
        assert!(is_tailscale_name(" Tailscale "));
        assert!(!is_tailscale_name("Ethernet"));

        assert!(is_tailscale_cgnat(Ipv4Addr::new(100, 64, 0, 1)));
        assert!(is_tailscale_cgnat(Ipv4Addr::new(100, 127, 255, 254)));
        assert!(!is_tailscale_cgnat(Ipv4Addr::new(100, 63, 255, 254)));
    }

    #[test]
    fn detects_likely_virtual_interfaces() {
        assert!(is_likely_virtual_interface("vEthernet (WSL)"));
        assert!(is_likely_virtual_interface("Docker Desktop Network"));
        assert!(is_likely_virtual_interface("VMware Network Adapter VMnet8"));
        assert!(!is_likely_virtual_interface("Wi-Fi"));
        assert!(!is_likely_virtual_interface("Ethernet"));
    }
}
