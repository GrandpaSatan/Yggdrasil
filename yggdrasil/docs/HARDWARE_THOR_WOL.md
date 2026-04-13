# Thor — Wake-on-LAN Debugging

Thor (Proxmox host for the Morrigan inference VM, ASUS Prime motherboard, Intel/Realtek NIC) is *configured* for WoL but does not respond to magic packets. This document is the runbook for diagnosing in person.

## Symptoms (from `ygg-gaming` logs, 2026-04-01 onward)

- `ygg-gaming launch` sends a magic packet via `etherwake`/UDP-port-9 from Munin (10.0.65.8) to Thor's MAC
- Magic packet leaves the workstation correctly (verified with `tcpdump -i eth0 'udp port 9'` on a sniffer)
- Thor stays powered off
- BIOS reports WoL "Enabled" under APM Configuration

## Diagnostic checklist (run in order until one fails)

### 1. Confirm BIOS settings (physical access required)

```
ASUS Prime BIOS → Advanced → APM Configuration:
  Power On By PCI-E/PCI    = ENABLED   (← required for WoL)
  ErP Ready                = DISABLED  (← if Enabled, blocks WoL)
  Restore AC Power Loss    = "Last State" or "Power On"
```

The `ErP Ready` setting is the most common silent killer — it cuts power to the NIC in S5 (soft-off) state to meet EU energy regulations.

### 2. Boot Linux on Thor (via USB stick is fine), check NIC supports WoL

```bash
# Identify the NIC
ip -br link

# Check WoL capability (g = magic packet wake)
sudo ethtool eth0 | grep -E "Wake-on|Supports Wake-on"

# Expected output:
#   Supports Wake-on: pumbg     ← g must be present
#   Wake-on: g                  ← g means magic-packet wake is currently enabled

# If "Wake-on" shows "d" (disabled), enable persistently:
sudo ethtool -s eth0 wol g

# Make persistent across reboots (Ubuntu/Debian):
sudo tee /etc/networkd-dispatcher/configured.d/wol.sh <<'EOF'
#!/bin/sh
ethtool -s eth0 wol g
EOF
sudo chmod +x /etc/networkd-dispatcher/configured.d/wol.sh
```

For Proxmox specifically, add to `/etc/network/interfaces`:
```
auto eth0
iface eth0 inet static
    address 10.0.65.X/24
    gateway 10.0.65.1
    post-up /usr/sbin/ethtool -s eth0 wol g
```

### 3. Verify the magic packet actually arrives at Thor's NIC

While Thor is **on**, run on Thor:
```bash
# Listen for any incoming WoL frames
sudo tcpdump -i eth0 'ether proto 0x0842 or udp port 9' -nn -e
```

Then from your workstation send a wake:
```bash
sudo etherwake -i eth0 <THOR_MAC>
# or with broadcast UDP:
wakeonlan -i 10.0.65.255 <THOR_MAC>
```

You should see the frame arrive at Thor with the `FF:FF:FF:FF:FF:FF` prefix followed by Thor's MAC repeated 16 times.

If the packet doesn't arrive:
- Check switch/router config — some L2 switches drop broadcast frames
- Try sending from the same broadcast domain (no router hop)
- Try `ether-wake` directly: `sudo etherwake <MAC>` from a host on Thor's subnet

### 4. After Thor goes to S5, verify NIC stays powered

After shutdown, *physically check* the NIC LEDs on Thor:
- LEDs **on** (steady green or amber) = NIC has power, can listen for WoL
- LEDs **off** = NIC powered down → WoL impossible regardless of BIOS settings

If LEDs are off:
- Re-check `ErP Ready` in BIOS (should be DISABLED)
- Some PSUs cut +5VSB to add-on cards under aggressive ErP — try a different PSU
- Some boards have a "Deep Sleep" jumper near the CMOS battery — remove if present

### 5. Switch/router-level diagnosis

```bash
# On the workstation, check ARP for Thor's MAC
arp -a | grep -i <thor-mac-prefix>

# If Thor's MAC isn't in ARP table when off, the switch may be aging it out
# Add a static ARP entry on the workstation:
sudo arp -s 10.0.65.<X> <THOR_MAC>
```

WoL packets need to reach Thor's MAC. Some switches age out MAC table entries after 5 min idle, then unicast WoL packets get flooded — usually fine, but if VLAN segmentation or storm control is in play, packets can be dropped.

### 6. Last resort — IPMI / iKVM / physical button

If WoL is fundamentally broken (motherboard age, NIC firmware bug), workarounds:

- **Smart plug** (Kasa, Sonoff): power-cycle the PSU; rely on "Restore on AC Loss = Power On" in BIOS to auto-boot. `ygg-gaming` could call the smart plug's API instead of WoL. Cleanest fix.
- **IPMI/iKVM card**: ASUS Prime models don't have onboard IPMI; an ASMB IPMI card or a Pi-based KVM (PiKVM) gives full out-of-band power control + console.
- **Physical wake**: the operator presses Thor's power button. Manual but reliable.

## Recording the fix

Once WoL works, store the outcome:

```bash
# From any node with the MCP server:
mcp__yggdrasil__store_memory_tool \
  --cause "Thor WoL fix" \
  --effect "<the actual fix that worked, with BIOS values + ethtool config>" \
  --tags "hardware,thor,wol,resolved"
```

Then update `MEMORY.md` to remove the open-issue line "Thor WoL not working — packets send correctly but Thor doesn't respond".

## Open since

2026-03-29 (per memory). 14 days as of this writing. Severity: medium — `ygg-gaming launch` requires manual power-on of Thor, breaking the on-demand inference pattern.
