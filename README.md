![LibreSpeed Logo](https://github.com/librespeed/speedtest/blob/master/.logo/logo3.png?raw=true)

# speedtest-rust

No Flash, No Java, No WebSocket, No Bullshit.

## Try it
[Take a speed test](https://librespeed-rs.ir)

## Compatibility
Compatible with all librespeed clients :

- Web Client
- [Command line client](https://github.com/librespeed/speedtest-cli)
- [Android client](https://github.com/librespeed/speedtest-android)
- [Desktop client](https://github.com/librespeed/speedtest-desktop)

## Attributes
- Memory safety (uses `#![forbid(unsafe_code)]` to ensure everything is implemented in 100% safe Rust.)
- Lightweight & Low overhead
- Low level networking
- Based on tokio-rs (asynchronous)

## Features
- Download
- Upload
- Ping
- Jitter
- IP Address, ISP
- Telemetry (optional)
- Results sharing (optional)

## Server requirements
- Any [Rust supported platforms](https://doc.rust-lang.org/beta/rustc/platform-support.html)
- PostgreSQL or MySQL database to store test results (optional)
- A fast! Internet connection

## Installation

### Install using prebuilt binaries

1. Download the appropriate binary file from the [releases](https://github.com/librespeed/speedtest-rust/releases/) page.
2. Unzip the archive.
3. Make changes to the configuration.
4. Run the binary.
5. Or setup as service :
    - Copy `setup_systemd.sh` on linux or `setup_sc_win.bat` on windows system in extracted folder.
    - Run the script file to setup as service

[Read full installation methods in wiki](https://github.com/librespeed/speedtest-rust/wiki/Installation)

### Stats page session secret

Stats page logins are held in a signed cookie. The signing key comes from, in order of precedence:

1. the `LIBRESPEED_STATS_SECRET` environment variable
2. `stats_secret_key` in `configs.toml`
3. a random key generated at startup, if neither of the above is set

There is no command line flag for the key, because argv is readable by any local user through `ps`.

Leaving the key unset is safe but ephemeral: every restart mints a new key and logs stats users
out. Set it to a fresh random value (64 characters or so) per deployment to keep sessions alive,
and never reuse a key that has been published anywhere.

## Note :
This project can be much better.\
Therefore, your PRs are accepted to improve and solve problems

## License
Copyright (C) 2016-2024 Federico Dossena\
Copyright (C) 2024-2026 Sudo Dios

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Lesser General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU Lesser General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/lgpl>.
