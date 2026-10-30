NEONTRACE 1.0.0
Native visual ping and route explorer
Copyright (c) 2026 Davoda1

SUPPORTED SYSTEMS
Windows 11 x64; Linux amd64 with an Ubuntu 24.04-equivalent or newer
baseline. Native Rust/Iced application. Modern integrated graphics are the
target; a dedicated graphics card is not required.

GETTING STARTED
Windows: extract the whole portable ZIP and open NeonTrace.exe at its root.
Linux portable: extract the tar.gz and open the neontrace binary at its root.
A desktop portal with a GTK or KDE FileChooser backend is needed for export
dialogs; the Debian package declares these runtime dependencies.
Debian/Ubuntu: install the .deb package with the system package manager and
open NeonTrace from the application menu. The only /usr/bin entry installed
by this package is the regular native executable /usr/bin/neontrace.

Enter a hostname, IPv4/IPv6 address, or URL. A pasted URL supplies only its
hostname; its page is not downloaded. Start pins the chosen resolved address,
begins endpoint measurements, and independently discovers an initial route.
Stop retains results for inspection. Trace again refreshes the snapshot.
Replay affects only the retained visualization. A detected local network route
change stops the current group and marks its origin stale; Start begins a fresh
session so observations from different networks are not stitched together.

Statistics and raw endpoint samples cover the latest 3,600 probes per target;
the latest 16 previous route snapshots are retained separately. Counts and
averages share that bounded sample window. History retains at most 100 saved
records and 32 MiB.

Concurrent mode configures one to ten validated targets. Its compact view
suppresses detailed bottom hop bars while retaining per-target measurements.
Switching back stops group work. Ordinary relaunch starts measurements stopped.

OFFLINE DEMONSTRATION
The labelled offline demonstration contains fictional observations. It sends
no network requests and requires no downloaded location database. The Preview
HTML is also a fictional, offline visual demonstration rather than a native
application or record of a real route.

MEASUREMENTS AND ANIMATION
Every complete displayed outbound journey lasts eight seconds. Green travel,
arrival effects, and single-target hop-bar fills share that presentation
timeline. Eight seconds is not a probe timeout, RTT, or discovery deadline.
There is no animation speed selector or reverse-path animation.

Endpoint ICMP measures round-trip delay. Separate TTL/hop-limit observations
can reveal responding interfaces; intermediate routers may not reply. A
nonresponding hop does not establish lost forwarded traffic. Each hop RTT is
a round trip from this device, not a delay of the preceding link. Do not sum
hop RTTs. Continuous ping does not prove that every request follows the last
snapshot; its timestamp and age remain relevant.

Locations are estimates. Schematic curves are not physical cable paths.
Unknown positions and missing replies remain separate states. CDN/anycast
endpoints may be an edge; no hidden origin route is inferred. The separate
optional TCP port 443 check is not ICMP or a full website health test.

APPEARANCE AND LOCALIZATION
The operating system controls light/dark appearance, including changes while
running; there is no override. The sole Simplemaps Robinson map is fitted
without cropping or distortion. English is the default; German, Hungarian,
French, Italian, and Russian are also provided.

DATA AND PRIVACY
First live use discovers the public network egress address through ipify
HTTPS and automatically downloads DB-IP Lite City and ASN databases without
an account or key. Target and hop addresses are looked up locally. DB-IP
receives database-download requests; ipify receives public-IP-discovery
requests. Neither provider receives target lists or saved route results.
The estimated origin may be an ISP/VPN/proxy gateway, not this device's
physical location. Failed discovery leaves origin unlocated.

Cached databases are checked monthly while the application is in use. A
failed update retains the last valid cache. Keep enough free disk space for
the current City/ASN datasets and replacement staging files.

Configuration uses the per-user application directory (%AppData% on Windows,
~/.config on Linux, respecting platform overrides). Database cache files are
kept separately. History is disabled by default; enable it deliberately to
retain sessions. JSON, CSV, and PNG exports are explicit local actions.
No telemetry, advertising, remote crash reporting, automatic application-
update checks, or permanent background service is included.

PERMISSIONS AND PRACTICAL LIMITS
The main GUI runs without elevation. Native diagnostic support depends on
socket permissions and the local network. Firewalls, target filtering,
rate-limiting, and missing IPv6 connectivity can yield incomplete results.
An incomplete route is not converted into a successful demonstration.
Do not run the whole GUI as root to conceal diagnostic permission errors.

RELEASE SET AND INTEGRITY
1. NeonTrace-1.0.0-Windows-x64.zip
2. NeonTrace-1.0.0-Linux-amd64.tar.gz
3. NeonTrace-1.0.0-Linux-amd64.deb
4. NeonTrace-1.0.0-Source.zip
5. NeonTrace-1.0.0-Source.tar.gz
6. NeonTrace-1.0.0-Preview.html
7. NeonTrace-1.0.0-README-Notes.txt
8. SHA256SUM.txt

The single SHA256SUM.txt covers the other seven release files. There are no
per-file checksum sidecars. Binary packages contain no launcher scripts.

LICENSE AND CREDITS
NeonTrace: MIT License. Copyright (c) 2026 Davoda1.
Map: https://simplemaps.com/resources/svg-world
Original map MIT notice: Copyright (c) 2020 Pareto Softare, LLC DBA
Simplemaps.com. The original spelling and complete notice are preserved.
The original 470 polygon parts and 220 named groups are retained. Antarctica
is outside the selected asset's supplied coverage.

IP Geolocation by DB-IP: https://db-ip.com/
DB-IP Lite: https://db-ip.com/db/lite.php
Data license: CC BY 4.0, https://creativecommons.org/licenses/by/4.0/
Lite data has reduced coverage and accuracy. Installed provider/edition
metadata is retained with cached data and relevant exports.

See LICENSE and THIRD-PARTY-NOTICES.txt inside each binary package for full
project, map, and compiled dependency notices.
