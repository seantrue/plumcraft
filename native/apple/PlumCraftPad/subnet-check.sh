#!/bin/bash
# Is IPv4 address $1 on one of this Mac's local networks (an interface's own subnet, not a tunnel:
# utun, ipsec, ppp, wg, gif, stf, or a point-to-point link)? Exit 0 and print the interface if so;
# exit 1 and print the Mac's subnets if not. Used by device.sh before it waits on an app that
# listens on the iPad's address (N4 slice 6: the iPad bound a VPN tunnel's 10.5.0.2, which the
# Mac can't reach, and the script waited five minutes).
ip="${1:?usage: subnet-check.sh IPV4}"
python3 - "$ip" <<'PY'
import ipaddress, re, subprocess, sys
want = ipaddress.ip_address(sys.argv[1])
out = subprocess.run(["ifconfig"], capture_output=True, text=True).stdout
nets, iface, flags = [], None, ""
for line in out.splitlines():
    m = re.match(r"^(\S+): flags=\S+<([^>]*)>", line)
    if m:
        iface, flags = m.group(1), m.group(2)
        continue
    m = re.match(r"^\s+inet (\d+\.\d+\.\d+\.\d+) (?:--> \S+ )?netmask (0x[0-9a-f]+)", line)
    if not m or iface is None:
        continue
    tunnel = re.match(r"^(utun|ipsec|ppp|wg|gif|stf|lo)", iface) or "POINTOPOINT" in flags.split(",")
    if tunnel:
        continue
    mask = int(m.group(2), 16)
    net = ipaddress.ip_network(f"{m.group(1)}/{bin(mask).count('1')}", strict=False)
    nets.append((iface, net))
for iface, net in nets:
    if want in net:
        print(f"{want} is on {iface}'s network {net}")
        sys.exit(0)
print(f"{want} is on none of this Mac's local networks: " + (", ".join(f"{i} {n}" for i, n in nets) or "none"))
sys.exit(1)
PY
