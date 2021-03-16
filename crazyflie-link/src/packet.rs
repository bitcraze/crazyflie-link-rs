use std::fmt;

pub struct Packet {
    port: u8,
    channel: u8,
    data: Vec<u8>,
}

impl fmt::Display for Packet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "(ch: {}, port: {}, data: {})",
            self.channel, self.port, self.data[0]
        )
    }
}

impl From<Vec<u8>> for Packet {
    fn from(v: Vec<u8>) -> Self {
        let header = v[0];
        //
        // A packet on the wire starts with a header byte, formatted as:
        //
        //   pppp00cc
        //
        // Where ...
        //  ... bit 1 and 2 is the channel (c) ...
        //  ... bit 3 and 4 are reserved for flow-control for the radio link ...
        //  ... bit 5, 6, 7 and 8 is the port (p).
        //
        let channel = header & 0x03; // mask out the channel
        let port = (header & 0xF0) >> 4; // twiddle out the port
        let data = v[1..].to_vec(); // the rest is data!

        Packet {
            port,
            channel,
            data,
        }
    }
}

impl From<Packet> for Vec<u8> {
    fn from(packet: Packet) -> Self {
        let mut vec = Vec::new();

        vec.push(packet.get_header());
        vec.append(&mut packet.get_data().to_vec());
        vec
    }
}

impl Packet {
    pub fn new(port: u8, channel: u8, data: Vec<u8>) -> Self {
        Packet {
            port,
            channel,
            data,
        }
    }

    pub fn new_from_header(header: u8) -> Self {
        Packet {
            port: (header & 0xF0) >> 4,
            channel: header & 0x03,
            data: Vec::new(),
        }
    }

    pub fn get_channel(&self) -> u8 {
        self.channel
    }

    pub fn get_port(&self) -> u8 {
        self.port
    }

    pub fn get_data(&self) -> &Vec<u8> {
        &self.data
    }

    pub fn append_data(&mut self, v: &mut Vec<u8>) {
        self.data.append(v);
    }

    pub fn get_header(&self) -> u8 {
        //
        // See the From trait implementation above for more details of the
        // vector format.
        //
        let mut header = 0;

        // channel is at bit 1 to 2
        header |= self.channel & 0x03; // header => 000000cc

        // port is at bit 5 to 8
        header | (self.port << 4) & 0xF0 // header => pppp00cc
    }
}
