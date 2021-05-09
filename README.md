# Crazyflie link implementation for rust

This repos contains an experimental implementation of the Crazyflie radio link in Rust as well as a python binding for it.

It allows to scan for Crazyflies and to open a safe bidirectional radio connection using a Crazyradio.

Look at and run the [examples](crazyflie-link/examples) to understand the current state of the link. Implementeation is still in progress.

The link is implemented in Rust Async functions, this means that an async executor needs to be used.
`async-std` is used in the examples and by the python binding.
`async_executors` is used to make the lib independent of specific executors,
any executors handles by `async_executors` is supported, this includes `tokio` and `wasm_bindgen_futures`.

## Working with the python binding

The python binding uses PyO3 and Maturin. You can test it in a venv:

```
# From within a venv, . ./venv/bin/activate ...
$ cd python-binding
$ maturin develop
$ python
>>> import cflinkrs
>>> context = cflinkrs.LinkContext()
>>> context.scan()
['radio://0/60/2M/E7E7E7E7E7']
>>>
```