# Simple example that scans for Crazyflies, connect the first found and print
# it the console.
# Press ctrl-C to terminate

import cflinkrs

ctx = cflinkrs.LinkContext()

found = ctx.scan()

if len(found) > 0:
    link = ctx.open_link(found[0])

    while True:
        packet = link.receive_packet()
        if packet and packet[0] == 0:
            print(bytes(packet[1:]).decode("utf8"), end='')
