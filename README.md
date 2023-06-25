<p align="center">
    <img src="https://i.imgur.com/76FWBbY.png" width="100px">
</p>

<h1 align="center">pisshoff</h1>

A very simple SSH server using [thrussh][] that exposes mocked versions of common `bash` commands
to act as a honeypot for would-be crackers.

All actions undertaken on the connection by the client are recorded in JSON format in an audit log
file.

[thrussh]: https://crates.io/crates/thrussh