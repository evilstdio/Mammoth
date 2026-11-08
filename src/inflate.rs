//! A small DEFLATE (RFC 1951) decoder for reading compressed JAR entries.

struct Bits<'a> {
    data: &'a [u8],
    position: usize,
    buffer: u64,
    count: u32,
}

impl Bits<'_> {
    fn need(&mut self, count: u32) -> Result<(), &'static str> {
        while self.count < count {
            let byte = *self
                .data
                .get(self.position)
                .ok_or("compressed data ended early")?;
            self.position += 1;
            self.buffer |= u64::from(byte) << self.count;
            self.count += 8;
        }
        Ok(())
    }

    fn take(&mut self, count: u32) -> Result<u32, &'static str> {
        if count == 0 {
            return Ok(0);
        }
        self.need(count)?;
        let value = (self.buffer & ((1u64 << count) - 1)) as u32;
        self.buffer >>= count;
        self.count -= count;
        Ok(value)
    }

    fn align(&mut self) {
        let extra = self.count % 8;
        self.buffer >>= extra;
        self.count -= extra;
    }
}

/// A canonical Huffman decoding table.
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, &'static str> {
        let mut counts = [0u16; 16];
        for &length in lengths {
            counts[length as usize] += 1;
        }
        counts[0] = 0;
        let mut offsets = [0u16; 16];
        for length in 1..16 {
            offsets[length] = offsets[length - 1] + counts[length - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                symbols[offsets[length as usize] as usize] = symbol as u16;
                offsets[length as usize] += 1;
            }
        }
        Ok(Self { counts, symbols })
    }

    fn decode(&self, bits: &mut Bits<'_>) -> Result<u16, &'static str> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for length in 1..16 {
            code |= bits.take(1)? as i32;
            let count = i32::from(self.counts[length]);
            if code - count < first {
                return Ok(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("invalid Huffman code")
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Decompresses raw DEFLATE data. `size_hint` preallocates the output.
pub fn inflate(data: &[u8], size_hint: usize) -> Result<Vec<u8>, &'static str> {
    let mut bits = Bits {
        data,
        position: 0,
        buffer: 0,
        count: 0,
    };
    let mut output = Vec::with_capacity(size_hint);
    loop {
        let last = bits.take(1)? == 1;
        match bits.take(2)? {
            0 => {
                bits.align();
                let length = bits.take(16)? as usize;
                let inverse = bits.take(16)? as usize;
                if length != !inverse & 0xffff {
                    return Err("invalid stored block length");
                }
                for _ in 0..length {
                    output.push(bits.take(8)? as u8);
                }
            }
            1 => {
                let mut lengths = [0u8; 288];
                lengths[..144].fill(8);
                lengths[144..256].fill(9);
                lengths[256..280].fill(7);
                lengths[280..].fill(8);
                let literals = Huffman::new(&lengths)?;
                let distances = Huffman::new(&[5u8; 30])?;
                block(&mut bits, &mut output, &literals, &distances)?;
            }
            2 => {
                let literal_count = bits.take(5)? as usize + 257;
                let distance_count = bits.take(5)? as usize + 1;
                let code_count = bits.take(4)? as usize + 4;
                const ORDER: [usize; 19] = [
                    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                ];
                let mut code_lengths = [0u8; 19];
                for &position in &ORDER[..code_count] {
                    code_lengths[position] = bits.take(3)? as u8;
                }
                let codes = Huffman::new(&code_lengths)?;
                let mut lengths = vec![0u8; literal_count + distance_count];
                let mut index = 0;
                while index < lengths.len() {
                    let symbol = codes.decode(&mut bits)?;
                    let (value, repeat) = match symbol {
                        0..=15 => (symbol as u8, 1),
                        16 => {
                            let previous = *lengths
                                .get(index.wrapping_sub(1))
                                .ok_or("repeat without a previous length")?;
                            (previous, 3 + bits.take(2)? as usize)
                        }
                        17 => (0, 3 + bits.take(3)? as usize),
                        _ => (0, 11 + bits.take(7)? as usize),
                    };
                    if index + repeat > lengths.len() {
                        return Err("too many code lengths");
                    }
                    lengths[index..index + repeat].fill(value);
                    index += repeat;
                }
                let literals = Huffman::new(&lengths[..literal_count])?;
                let distances = Huffman::new(&lengths[literal_count..])?;
                block(&mut bits, &mut output, &literals, &distances)?;
            }
            _ => return Err("invalid block type"),
        }
        if last {
            return Ok(output);
        }
    }
}

fn block(
    bits: &mut Bits<'_>,
    output: &mut Vec<u8>,
    literals: &Huffman,
    distances: &Huffman,
) -> Result<(), &'static str> {
    loop {
        let symbol = literals.decode(bits)?;
        match symbol {
            0..=255 => output.push(symbol as u8),
            256 => return Ok(()),
            _ => {
                let index = usize::from(symbol - 257);
                if index >= LENGTH_BASE.len() {
                    return Err("invalid length symbol");
                }
                let length = usize::from(LENGTH_BASE[index])
                    + bits.take(u32::from(LENGTH_EXTRA[index]))? as usize;
                let code = usize::from(distances.decode(bits)?);
                if code >= DISTANCE_BASE.len() {
                    return Err("invalid distance symbol");
                }
                let distance = usize::from(DISTANCE_BASE[code])
                    + bits.take(u32::from(DISTANCE_EXTRA[code]))? as usize;
                if distance > output.len() {
                    return Err("distance is too far back");
                }
                let start = output.len() - distance;
                for offset in 0..length {
                    let byte = output[start + offset];
                    output.push(byte);
                }
            }
        }
    }
}
