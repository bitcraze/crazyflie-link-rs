# Crazyflie link

Radio link implementation for the Crazyflie quadcopter.

This crates implements low-level CRTP communication to a [Crazyflie](https://www.bitcraze.io/products/crazyflie-2-1/) using the [Crazyradio](https://www.bitcraze.io/products/crazyradio-pa/) dongle.

This crate API is async, the [async_executor](https://crates.io/crates/async_executors)
crate is used to abstract the async executor. Examples are using `async-std`.