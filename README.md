# NeonTrace

NeonTrace is a native Rust desktop application for viewing endpoint ping measurements and route observations on a large world map. Green outbound journeys use a fixed eight-second presentation timeline; displayed RTT values come from network probes.


![NeonTrace offline demonstration with fictional observations](docs/extras/poster.png)

## Downloads

[NeonTrace 1.0.0 release files](https://github.com/Davoda1/Neon-Trace/releases/tag/v1.0.0) include the Windows portable ZIP, Linux tar.gz and Debian package, both source archives, offline Preview HTML, README/Notes, and one SHA256SUM file. [Build and test results](https://github.com/Davoda1/Neon-Trace/actions/workflows/verify.yml).

## Operation

Enter a hostname, IP address, or URL and select **Start**. A URL contributes its hostname; NeonTrace does not fetch its page. The selected resolved address stays pinned during the session. Endpoint ping and initial route discovery run independently. **Stop** cancels work and retains observations for inspection; **Trace again** refreshes the route snapshot; **Replay** animates retained observations without sending probes.

The single-target view includes continuously filling hop-transition bars. Concurrent mode accepts one to ten targets in a popup, validates each entry, removes exact duplicates, and uses compact target summaries without the detailed bottom strip. Switching back stops group measurements and restores the retained single-target view.

**Offline demonstration** uses fictional embedded observations. It does not discover a public address, download databases, resolve hosts, or send diagnostic probes.

## Reading the map

- Curves connect estimated locations schematically. They are not cable paths.
- Ping measures a round trip to one endpoint. It does not reveal intermediate routers.
- Each hop RTT is a round trip from this computer to the responding hop. Hop RTTs are never added or subtracted to estimate link latency.
- A missing intermediate reply can reflect filtering or rate limiting. It does not prove that forwarded traffic was lost.
- Unknown geographical positions remain unknown. Country-only data does not become a city pin.
- A CDN or anycast address can identify an edge endpoint. No hidden origin connection is inferred.
- The optional TCP connection to port 443 is a separate connectivity check. It is not ICMP ping or a full website test.

## Appearance and languages

NeonTrace follows the operating system's light/dark preference, including changes while running. There is no manual theme selector. Its sole map is the supplied Simplemaps Robinson world map, fitted uniformly with all supplied geography visible. Journey duration is fixed in code.

English is the default. German, Hungarian, French, Italian, and Russian are also available. The window is designed around 1280 × 800 and supports operation down to approximately 680 × 540. Details and dialogs scroll within their panels.

## Data and privacy

First live use automatically obtains the public network egress address through ipify over HTTPS and downloads the free DB-IP Lite City and ASN databases. Database downloads and validation run outside drawing. Updates are checked monthly while the application is in use; failed updates retain the last valid cache. No account or service key is required.

Target and hop IP addresses are looked up locally. They are not sent to an online geolocation API. A public egress estimate can represent an ISP, VPN, proxy, or carrier gateway rather than the computer's physical position.

Local history is disabled by default. Settings do not silently store queried targets or complete sessions. Explicit exports can contain the selected targets and observations. No telemetry, advertising, remote crash reporting, permanent background service, or automatic application-update checks are included.

Configuration uses the per-user application configuration directory, respecting platform overrides. Downloaded databases are kept separately in the per-user cache/data location. Ordinary startup begins with measurement stopped.

## Native packages

Supported targets are Windows 11 x64 and Linux amd64 with an Ubuntu 24.04-equivalent or newer baseline. A dedicated graphics card is not required. Driver and display-server behavior still depends on the installed system.

The portable Windows ZIP contains the real `NeonTrace.exe` at archive root. Extract all files together and open that executable. The Linux tar.gz contains the real `neontrace` binary at archive root with executable permissions. The Debian package installs one regular executable at `/usr/bin/neontrace`; it does not install a launcher or symlink there. Binary packages contain no `.sh`, `.cmd`, or `.ps1` launchers.

The GUI remains unprivileged. Available diagnostic behavior depends on the operating system, local socket permissions, firewall, and target response policy. A real error or incomplete route remains visible; do not run the entire application as root to hide a limitation.

## Build from source

Install Rust 1.99.1 and the native build dependencies for the target system. The complete dependency graph is fixed by `Cargo.lock`.

```console
cargo build --release --locked
cargo test --locked
```

On Ubuntu 24.04, development dependencies include a C toolchain, `pkg-config`, the X11/Wayland and xkbcommon development libraries, and Vulkan runtime support. For Windows MSVC, use the Visual Studio C++ build tools and Windows SDK so `rc.exe` is available. The GNU cross-target uses `x86_64-w64-mingw32-gcc` and `x86_64-w64-mingw32-windres`.

`tools/dependency_notices.py` collects exact notices from the downloaded locked crate sources. `tools/package.py` prepares the eight-file release set, verifies native file signatures, checks for exposed credentials, checks the Linux glibc baseline, and inspects the Debian `/usr/bin` entry. Packaging does not publish anything.

## Attribution

NeonTrace is MIT licensed: **Copyright (c) 2026 Davoda1**.

The selected world geometry comes from [Simplemaps](https://simplemaps.com/resources/svg-world), under its original MIT notice: Copyright (c) 2020 Pareto Softare, LLC DBA Simplemaps.com. The upstream spelling is preserved. The asset contains 470 original polygon parts in 220 named country/territory groups; Antarctica is outside its supplied coverage.

[IP Geolocation by DB-IP](https://db-ip.com/). DB-IP Lite City and ASN data are licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). The free edition has reduced coverage and accuracy. Installed edition metadata is retained with cached data and relevant exports.

See `LICENSE`, `assets/SIMPLEMAPS-LICENSE.txt`, and `docs/THIRD-PARTY-NOTICES.txt` for distributed license and dependency notices.
