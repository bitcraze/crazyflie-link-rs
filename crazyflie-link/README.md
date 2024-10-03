# Crazyflie link

Link implementation for the Crazyflie quadcopter.

This crates implements low-level link communication to a [Crazyflie] using the
[Crazyradio] dongle or the direct USB connection. It allows to scan for Crazyflies and to open a
bidirectional link.


This crate API is async, it is implemented using the Tokio executor.

## Limitations

This crate currently only supports 2Mbit/s datarate over Crazyradio.

This crate currently only supports the Tokio executor.

[Crazyflie]: https://www.bitcraze.io/products/crazyflie-2-1/
[Crazyradio]: https://www.bitcraze.io/products/crazyradio-pa/
