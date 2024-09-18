# Crazyflie link

Radio link implementation for the Crazyflie quadcopter.

This crates implements low-level link communication to a [Crazyflie] using the
[Crazyradio] dongle. It allows to scan for Crazyflies and to open a safe
bidirectional radio connection.


This crate API is async, it is implemented using the Tokio executor.

## Limitations

This crate currently only supports 2Mbit/s datarate.

[Crazyflie]: https://www.bitcraze.io/products/crazyflie-2-1/
[Crazyradio]: https://www.bitcraze.io/products/crazyradio-pa/
[async_executor]: https://crates.io/crates/async_executors
[Crazyradio crate]: https://crates.io/crates/crazyradio
[Crazyradio-webusb crate]: https://crates.io/crates/crazyradio-webusb