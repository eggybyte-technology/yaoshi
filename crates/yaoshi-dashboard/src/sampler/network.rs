fn read_address_map() -> Result<BTreeMap<String, Vec<IpAddr>>, std::io::Error> {
    let ifindex_names = read_ifindex_name_map();
    let mut out = read_rtnetlink_address_map(&ifindex_names)?;
    for addresses in out.values_mut() {
        addresses.retain(|ip| !ip.is_loopback());
        sort_addresses(addresses);
        addresses.dedup();
    }
    out.retain(|_, addresses| !addresses.is_empty());
    Ok(out)
}

fn read_ifindex_name_map() -> BTreeMap<u32, String> {
    let mut out = BTreeMap::new();
    let Ok(entries) = fs::read_dir("/sys/class/net") else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Ok(raw) = read_trimmed(entry.path().join("ifindex"))
            && let Ok(index) = raw.parse::<u32>()
        {
            out.insert(index, name);
        }
    }
    out
}

fn read_rtnetlink_address_map(
    ifindex_names: &BTreeMap<u32, String>,
) -> Result<BTreeMap<String, Vec<IpAddr>>, std::io::Error> {
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            libc::NETLINK_ROUTE,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let result = read_rtnetlink_address_map_fd(fd, ifindex_names);
    unsafe {
        libc::close(fd);
    }
    result
}

fn read_rtnetlink_address_map_fd(
    fd: libc::c_int,
    ifindex_names: &BTreeMap<u32, String>,
) -> Result<BTreeMap<String, Vec<IpAddr>>, std::io::Error> {
    let mut bind_addr = unsafe { std::mem::zeroed::<libc::sockaddr_nl>() };
    bind_addr.nl_family = libc::AF_NETLINK as libc::sa_family_t;
    bind_addr.nl_pid = 0;
    bind_addr.nl_groups = 0;
    let bind_rc = unsafe {
        libc::bind(
            fd,
            (&bind_addr as *const libc::sockaddr_nl).cast(),
            size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if bind_rc < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let request = NetlinkAddrRequest::new();
    let sent = unsafe {
        libc::send(
            fd,
            (&request as *const NetlinkAddrRequest).cast(),
            size_of::<NetlinkAddrRequest>(),
            0,
        )
    };
    if sent < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let mut out: BTreeMap<String, Vec<IpAddr>> = BTreeMap::new();
    let mut buf = vec![0u8; 65_536];
    loop {
        let len = unsafe { libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0) };
        if len < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if len == 0 {
            break;
        }
        if parse_rtnetlink_address_messages(&buf[..len as usize], ifindex_names, &mut out)? {
            break;
        }
    }
    Ok(out)
}

#[repr(C)]
struct NetlinkAddrRequest {
    header: libc::nlmsghdr,
    message: IfAddrMsg,
}

impl NetlinkAddrRequest {
    fn new() -> Self {
        Self {
            header: libc::nlmsghdr {
                nlmsg_len: size_of::<NetlinkAddrRequest>() as u32,
                nlmsg_type: libc::RTM_GETADDR,
                nlmsg_flags: (libc::NLM_F_REQUEST | libc::NLM_F_DUMP) as u16,
                nlmsg_seq: 1,
                nlmsg_pid: 0,
            },
            message: IfAddrMsg {
                ifa_family: libc::AF_UNSPEC as u8,
                ifa_prefixlen: 0,
                ifa_flags: 0,
                ifa_scope: 0,
                ifa_index: 0,
            },
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct IfAddrMsg {
    ifa_family: u8,
    ifa_prefixlen: u8,
    ifa_flags: u8,
    ifa_scope: u8,
    ifa_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RtAttr {
    rta_len: u16,
    rta_type: u16,
}

fn parse_rtnetlink_address_messages(
    bytes: &[u8],
    ifindex_names: &BTreeMap<u32, String>,
    out: &mut BTreeMap<String, Vec<IpAddr>>,
) -> Result<bool, std::io::Error> {
    let mut offset = 0usize;
    while offset + size_of::<libc::nlmsghdr>() <= bytes.len() {
        let header = unsafe { read_unaligned_at::<libc::nlmsghdr>(bytes, offset)? };
        if header.nlmsg_len < size_of::<libc::nlmsghdr>() as u32 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid rtnetlink message length",
            ));
        }
        let msg_len = header.nlmsg_len as usize;
        if offset + msg_len > bytes.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "truncated rtnetlink message",
            ));
        }
        match header.nlmsg_type {
            value if value == libc::NLMSG_DONE as u16 => return Ok(true),
            value if value == libc::NLMSG_ERROR as u16 => {
                return Err(std::io::Error::other("rtnetlink address dump failed"));
            }
            libc::RTM_NEWADDR => {
                parse_rtnetlink_address_message(
                    &bytes[offset + size_of::<libc::nlmsghdr>()..offset + msg_len],
                    ifindex_names,
                    out,
                )?;
            }
            _ => {}
        }
        offset += align4(msg_len);
    }
    Ok(false)
}

fn parse_rtnetlink_address_message(
    bytes: &[u8],
    ifindex_names: &BTreeMap<u32, String>,
    out: &mut BTreeMap<String, Vec<IpAddr>>,
) -> Result<(), std::io::Error> {
    if bytes.len() < size_of::<IfAddrMsg>() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "truncated rtnetlink address payload",
        ));
    }
    let message = unsafe { read_unaligned_at::<IfAddrMsg>(bytes, 0)? };
    let Some(name) = ifindex_names.get(&message.ifa_index) else {
        return Ok(());
    };
    let mut attr_offset = align4(size_of::<IfAddrMsg>());
    while attr_offset + size_of::<RtAttr>() <= bytes.len() {
        let attr = unsafe { read_unaligned_at::<RtAttr>(bytes, attr_offset)? };
        if attr.rta_len < size_of::<RtAttr>() as u16 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid rtnetlink attribute length",
            ));
        }
        let attr_len = attr.rta_len as usize;
        if attr_offset + attr_len > bytes.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "truncated rtnetlink attribute",
            ));
        }
        let payload_start = attr_offset + size_of::<RtAttr>();
        let payload = &bytes[payload_start..attr_offset + attr_len];
        if matches!(attr.rta_type, libc::IFA_LOCAL | libc::IFA_ADDRESS)
            && let Some(ip) = parse_rtnetlink_ip(message.ifa_family, payload)
        {
            out.entry(name.clone()).or_default().push(ip);
        }
        attr_offset += align4(attr_len);
    }
    Ok(())
}

fn parse_rtnetlink_ip(family: u8, payload: &[u8]) -> Option<IpAddr> {
    match family as i32 {
        libc::AF_INET if payload.len() >= 4 => Some(IpAddr::V4(Ipv4Addr::new(
            payload[0], payload[1], payload[2], payload[3],
        ))),
        libc::AF_INET6 if payload.len() >= 16 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&payload[..16]);
            Some(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        _ => None,
    }
}

unsafe fn read_unaligned_at<T: Copy>(bytes: &[u8], offset: usize) -> Result<T, std::io::Error> {
    if offset + size_of::<T>() > bytes.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "truncated rtnetlink structure",
        ));
    }
    Ok(unsafe { std::ptr::read_unaligned(bytes.as_ptr().add(offset).cast::<T>()) })
}

fn align4(value: usize) -> usize {
    (value + 3) & !3
}

fn address_summary(addresses: &[IpAddr]) -> String {
    match addresses {
        [] => "no address".to_string(),
        [one] => one.to_string(),
        [first, rest @ ..] => format!("{}, +{}", first, rest.len()),
    }
}

fn access_address(ifaces: &[InterfaceInfo]) -> String {
    if ifaces.iter().any(|iface| iface.addresses_unavailable) {
        return "unavailable".to_string();
    }
    let mut candidates = Vec::new();
    for iface in ifaces {
        for address in &iface.addresses {
            if unusable_access_address(address) {
                continue;
            }
            candidates.push((
                kind_priority(&iface.kind),
                address_family_priority(address, libc::AF_INET as u8),
                address_scope_priority(address),
                iface.name.clone(),
                address_sort_key(address),
                *address,
            ));
        }
    }
    candidates.sort();
    candidates
        .first()
        .map(|(_, _, _, _, _, addr)| addr.to_string())
        .unwrap_or_else(|| {
            if ifaces.is_empty() {
                "unavailable".to_string()
            } else {
                "configuring".to_string()
            }
        })
}

fn access_summary(ifaces: &[InterfaceInfo], access_address: &str) -> String {
    if matches!(access_address, "unavailable" | "configuring") {
        return access_address.to_string();
    }
    let mut names = ifaces
        .iter()
        .filter(|iface| {
            iface
                .addresses
                .iter()
                .any(|addr| addr.to_string() == access_address)
        })
        .map(|iface| iface.name.clone())
        .collect::<Vec<_>>();
    if names.is_empty() {
        return unavailable();
    }
    names.sort();
    names.dedup();
    if names.len() == 1 {
        names.remove(0)
    } else {
        format!("{} +{}", names[0], names.len() - 1)
    }
}

fn selected_default_route(ifaces: &[InterfaceInfo]) -> Option<DefaultRoute> {
    let ifindex_names = ifaces
        .iter()
        .map(|iface| (iface.ifindex, iface.name.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut routes = read_default_routes_rtnetlink(&ifindex_names).ok()?;
    routes.sort_by(|a, b| {
        a.metric
            .unwrap_or(u32::MAX)
            .cmp(&b.metric.unwrap_or(u32::MAX))
            .then_with(|| {
                address_family_priority_for_family(a.family)
                    .cmp(&address_family_priority_for_family(b.family))
            })
            .then_with(|| a.iface.cmp(&b.iface))
            .then_with(|| route_gateway_key(&a.gateway).cmp(&route_gateway_key(&b.gateway)))
            .then_with(|| a.protocol.cmp(&b.protocol))
    });
    routes.into_iter().next()
}

fn route_address_for(iface: &str, family: u8, ifaces: &[InterfaceInfo]) -> Option<String> {
    let iface = ifaces.iter().find(|candidate| candidate.name == iface)?;
    let mut addresses = iface.addresses.clone();
    addresses.sort_by_key(|address| {
        (
            address_family_priority(address, family),
            address_scope_priority(address),
            address_sort_key(address),
        )
    });
    addresses.first().map(ToString::to_string)
}

fn route_rows_from_selected(
    route: &Option<DefaultRoute>,
    route_address: &str,
) -> Vec<DashboardRouteRow> {
    match route {
        Some(route) => vec![DashboardRouteRow {
            gateway: route
                .gateway
                .map(|gateway| gateway.to_string())
                .unwrap_or_else(|| "direct".to_string()),
            iface: route.iface.clone(),
            address: route_address.to_string(),
            metric: route
                .metric
                .map(|metric| metric.to_string())
                .unwrap_or_else(unavailable),
        }],
        None => Vec::new(),
    }
}

#[derive(Clone)]
struct DefaultRoute {
    iface: String,
    gateway: Option<IpAddr>,
    metric: Option<u32>,
    family: u8,
    protocol: u8,
}

fn read_default_routes_rtnetlink(
    ifindex_names: &BTreeMap<u32, String>,
) -> Result<Vec<DefaultRoute>, std::io::Error> {
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            libc::NETLINK_ROUTE,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let result = read_default_routes_rtnetlink_fd(fd, ifindex_names);
    unsafe {
        libc::close(fd);
    }
    result
}

fn read_default_routes_rtnetlink_fd(
    fd: libc::c_int,
    ifindex_names: &BTreeMap<u32, String>,
) -> Result<Vec<DefaultRoute>, std::io::Error> {
    let mut bind_addr = unsafe { std::mem::zeroed::<libc::sockaddr_nl>() };
    bind_addr.nl_family = libc::AF_NETLINK as libc::sa_family_t;
    let bind_rc = unsafe {
        libc::bind(
            fd,
            (&bind_addr as *const libc::sockaddr_nl).cast(),
            size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if bind_rc < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let request = NetlinkRouteRequest::new();
    let sent = unsafe {
        libc::send(
            fd,
            (&request as *const NetlinkRouteRequest).cast(),
            size_of::<NetlinkRouteRequest>(),
            0,
        )
    };
    if sent < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut out = Vec::new();
    let mut buf = vec![0u8; 65_536];
    loop {
        let len = unsafe { libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0) };
        if len < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if len == 0 {
            break;
        }
        if parse_rtnetlink_route_messages(&buf[..len as usize], ifindex_names, &mut out)? {
            break;
        }
    }
    Ok(out)
}

#[repr(C)]
struct NetlinkRouteRequest {
    header: libc::nlmsghdr,
    message: RtMsg,
}

impl NetlinkRouteRequest {
    fn new() -> Self {
        Self {
            header: libc::nlmsghdr {
                nlmsg_len: size_of::<NetlinkRouteRequest>() as u32,
                nlmsg_type: libc::RTM_GETROUTE,
                nlmsg_flags: (libc::NLM_F_REQUEST | libc::NLM_F_DUMP) as u16,
                nlmsg_seq: 1,
                nlmsg_pid: 0,
            },
            message: RtMsg {
                rtm_family: libc::AF_UNSPEC as u8,
                rtm_dst_len: 0,
                rtm_src_len: 0,
                rtm_tos: 0,
                rtm_table: 0,
                rtm_protocol: 0,
                rtm_scope: 0,
                rtm_type: 0,
                rtm_flags: 0,
            },
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RtMsg {
    rtm_family: u8,
    rtm_dst_len: u8,
    rtm_src_len: u8,
    rtm_tos: u8,
    rtm_table: u8,
    rtm_protocol: u8,
    rtm_scope: u8,
    rtm_type: u8,
    rtm_flags: u32,
}

fn parse_rtnetlink_route_messages(
    bytes: &[u8],
    ifindex_names: &BTreeMap<u32, String>,
    out: &mut Vec<DefaultRoute>,
) -> Result<bool, std::io::Error> {
    let mut offset = 0usize;
    while offset + size_of::<libc::nlmsghdr>() <= bytes.len() {
        let header = unsafe { read_unaligned_at::<libc::nlmsghdr>(bytes, offset)? };
        if header.nlmsg_len < size_of::<libc::nlmsghdr>() as u32 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid rtnetlink route message length",
            ));
        }
        let msg_len = header.nlmsg_len as usize;
        if offset + msg_len > bytes.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "truncated rtnetlink route message",
            ));
        }
        match header.nlmsg_type {
            value if value == libc::NLMSG_DONE as u16 => return Ok(true),
            value if value == libc::NLMSG_ERROR as u16 => {
                return Err(std::io::Error::other("rtnetlink route dump failed"));
            }
            libc::RTM_NEWROUTE => parse_rtnetlink_route_message(
                &bytes[offset + size_of::<libc::nlmsghdr>()..offset + msg_len],
                ifindex_names,
                out,
            )?,
            _ => {}
        }
        offset += align4(msg_len);
    }
    Ok(false)
}

fn parse_rtnetlink_route_message(
    bytes: &[u8],
    ifindex_names: &BTreeMap<u32, String>,
    out: &mut Vec<DefaultRoute>,
) -> Result<(), std::io::Error> {
    if bytes.len() < size_of::<RtMsg>() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "truncated rtnetlink route payload",
        ));
    }
    let message = unsafe { read_unaligned_at::<RtMsg>(bytes, 0)? };
    const RT_TABLE_MAIN: u8 = 254;
    const RTA_OIF: u16 = 4;
    const RTA_GATEWAY: u16 = 5;
    const RTA_PRIORITY: u16 = 6;
    const RTA_TABLE: u16 = 15;
    if message.rtm_dst_len != 0 {
        return Ok(());
    }
    let mut table = message.rtm_table as u32;
    let mut oif = None;
    let mut gateway = None;
    let mut metric = None;
    let mut offset = align4(size_of::<RtMsg>());
    while offset + size_of::<RtAttr>() <= bytes.len() {
        let attr = unsafe { read_unaligned_at::<RtAttr>(bytes, offset)? };
        if attr.rta_len < size_of::<RtAttr>() as u16 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid rtnetlink route attribute length",
            ));
        }
        let attr_len = attr.rta_len as usize;
        if offset + attr_len > bytes.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "truncated rtnetlink route attribute",
            ));
        }
        let payload = &bytes[offset + size_of::<RtAttr>()..offset + attr_len];
        match attr.rta_type {
            RTA_OIF => oif = read_u32_payload(payload),
            RTA_GATEWAY => gateway = parse_rtnetlink_ip(message.rtm_family, payload),
            RTA_PRIORITY => metric = read_u32_payload(payload),
            RTA_TABLE => table = read_u32_payload(payload).unwrap_or(table),
            _ => {}
        }
        offset += align4(attr_len);
    }
    if table != u32::from(RT_TABLE_MAIN) {
        return Ok(());
    }
    let Some(iface) = oif.and_then(|index| ifindex_names.get(&index).cloned()) else {
        return Ok(());
    };
    out.push(DefaultRoute {
        iface,
        gateway,
        metric,
        family: message.rtm_family,
        protocol: message.rtm_protocol,
    });
    Ok(())
}

fn measuring_rate_tuple8() -> (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
) {
    let value = "measuring".to_string();
    (
        value.clone(),
        value.clone(),
        value.clone(),
        value.clone(),
        value.clone(),
        value.clone(),
        value.clone(),
        value,
    )
}

fn unavailable_rate_tuple8() -> (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
) {
    let value = "unavailable".to_string();
    (
        value.clone(),
        value.clone(),
        value.clone(),
        value.clone(),
        value.clone(),
        value.clone(),
        value.clone(),
        value,
    )
}

fn sort_addresses(addresses: &mut [IpAddr]) {
    addresses.sort_by_key(address_sort_key);
}

fn address_sort_key(address: &IpAddr) -> (u8, [u8; 16]) {
    match address {
        IpAddr::V4(v4) => {
            let mut bytes = [0u8; 16];
            bytes[..4].copy_from_slice(&v4.octets());
            (0, bytes)
        }
        IpAddr::V6(v6) => (1, v6.octets()),
    }
}

fn unusable_access_address(address: &IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => v4.is_unspecified() || v4.is_loopback() || v4.is_multicast(),
        IpAddr::V6(v6) => v6.is_unspecified() || v6.is_loopback() || v6.is_multicast(),
    }
}

fn address_family_priority(address: &IpAddr, preferred_family: u8) -> u8 {
    let family = match address {
        IpAddr::V4(_) => libc::AF_INET as u8,
        IpAddr::V6(_) => libc::AF_INET6 as u8,
    };
    if family == preferred_family {
        0
    } else if matches!(address, IpAddr::V4(_)) {
        1
    } else {
        2
    }
}

fn address_family_priority_for_family(family: u8) -> u8 {
    if family == libc::AF_INET as u8 {
        0
    } else if family == libc::AF_INET6 as u8 {
        1
    } else {
        2
    }
}

fn address_scope_priority(address: &IpAddr) -> u8 {
    match address {
        IpAddr::V4(v4) if v4.is_link_local() => 1,
        IpAddr::V6(v6) if v6.is_unicast_link_local() => 1,
        _ => 0,
    }
}

fn kind_priority(kind: &str) -> u8 {
    match kind {
        "wireguard" => 0,
        "tunnel" => 1,
        "physical" => 2,
        "bridge" => 3,
        "bond" => 4,
        "vlan" => 5,
        "veth" => 6,
        "virtual" => 7,
        _ => 8,
    }
}

fn route_gateway_key(gateway: &Option<IpAddr>) -> (u8, [u8; 16]) {
    gateway
        .as_ref()
        .map(address_sort_key)
        .unwrap_or((0, [0; 16]))
}

fn read_u32_payload(payload: &[u8]) -> Option<u32> {
    let bytes: [u8; 4] = payload.get(..4)?.try_into().ok()?;
    Some(u32::from_ne_bytes(bytes))
}

fn interface_role(name: &str, route_iface: &str, access_summary: &str) -> String {
    let is_route = name == route_iface;
    let is_access = access_summary == name || access_summary.starts_with(&format!("{name} +"));
    match (is_route, is_access) {
        (true, true) => "route+access",
        (true, false) => "route",
        (false, true) => "access",
        (false, false) => "iface",
    }
    .to_string()
}

fn interface_alert_state(rx_err: &str, tx_err: &str, rx_drop: &str, tx_drop: &str) -> String {
    network_error_state(rx_err, tx_err, rx_drop, tx_drop)
}

fn interface_state(sysfs: &Path) -> String {
    match read_trimmed(sysfs.join("operstate")) {
        Ok(state)
            if matches!(
                state.as_str(),
                "up" | "down" | "lowerlayerdown" | "dormant" | "notpresent"
            ) =>
        {
            state
        }
        Ok(_) => "configuring".to_string(),
        Err(_) => "unavailable".to_string(),
    }
}

fn interface_kind(name: &str, sysfs: &Path) -> String {
    if name.starts_with("wg") || name.starts_with("wt") {
        "wireguard"
    } else if name.starts_with("tun") || name.starts_with("tap") {
        "tunnel"
    } else if sysfs.join("bridge").exists() {
        "bridge"
    } else if sysfs.join("bonding").exists() {
        "bond"
    } else if Path::new("/proc/net/vlan").join(name).exists() {
        "vlan"
    } else if name.starts_with("veth") {
        "veth"
    } else if sysfs.join("device").exists() {
        "physical"
    } else {
        "virtual"
    }
    .to_string()
}

fn interface_speed(sysfs: &Path) -> String {
    read_trimmed(sysfs.join("speed"))
        .ok()
        .and_then(|raw| raw.parse::<i64>().ok())
        .filter(|speed| *speed > 0)
        .map(|speed| format!("{speed} Mb/s"))
        .unwrap_or_else(unavailable)
}

fn excluded_disk(name: &str) -> bool {
    name.starts_with("ram")
        || name.starts_with("loop")
        || name.starts_with("dm-")
        || name.starts_with("md")
        || name.starts_with("zram")
        || name.starts_with("sr")
        || name.starts_with("fd")
}

fn disk_transport(name: &str, sysfs: &Path) -> String {
    if name.starts_with("nvme") {
        "nvme".to_string()
    } else if name.starts_with("vd") {
        "virtio".to_string()
    } else if name.starts_with("sd") {
        "sata/scsi".to_string()
    } else if name.starts_with("mmcblk") {
        "mmc".to_string()
    } else if disk_sysfs_mentions_usb(sysfs) {
        "usb".to_string()
    } else {
        "other".to_string()
    }
}

fn disk_sysfs_mentions_usb(sysfs: &Path) -> bool {
    fs::canonicalize(sysfs.join("device"))
        .ok()
        .is_some_and(|path| {
            path.components()
                .any(|component| component.as_os_str() == "usb")
        })
}
