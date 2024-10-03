# Crazyflie link implementation for rust

This repos contains an experimental implementation of the Crazyflie radio link in Rust as well as a python binding for it.

It allows to scan for Crazyflies and to open a safe bidirectional radio connection using a Crazyradio.

Look at and run the [examples](crazyflie-link/examples) to understand the current state of the link. Implementation is still in progress.

The link is implemented in Rust Async, it currently requires the Tokio executor.

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
