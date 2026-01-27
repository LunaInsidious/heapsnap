use std::io::{self, Read};

pub struct LenientJsonReader<'a, R: Read> {
    inner: &'a mut R,
    input: Vec<u8>,
    input_pos: usize,
    output: Vec<u8>,
    in_string: bool,
    escape: bool,
    eof: bool,
}

impl<'a, R: Read> LenientJsonReader<'a, R> {
    pub fn new(inner: &'a mut R) -> Self {
        Self {
            inner,
            input: Vec::with_capacity(8192),
            input_pos: 0,
            output: Vec::with_capacity(8192),
            in_string: false,
            escape: false,
            eof: false,
        }
    }

    fn ensure_available(&mut self, needed: usize) -> io::Result<bool> {
        while self.input.len().saturating_sub(self.input_pos) < needed && !self.eof {
            let mut buf = [0u8; 8192];
            let read = self.inner.read(&mut buf)?;
            if read == 0 {
                self.eof = true;
                break;
            }
            self.input.extend_from_slice(&buf[..read]);
        }
        Ok(self.input.len().saturating_sub(self.input_pos) >= needed)
    }

    fn peek(&mut self, needed: usize) -> io::Result<Option<&[u8]>> {
        if !self.ensure_available(needed)? {
            return Ok(None);
        }
        Ok(Some(&self.input[self.input_pos..self.input_pos + needed]))
    }

    fn consume(&mut self, count: usize) {
        self.input_pos += count;
        if self.input_pos > 8192 && self.input_pos > self.input.len() / 2 {
            self.input.drain(0..self.input_pos);
            self.input_pos = 0;
        }
    }

    fn process(&mut self) -> io::Result<()> {
        self.output.clear();

        while self.output.len() < 8192 {
            if !self.ensure_available(1)? {
                break;
            }

            let byte = self.input[self.input_pos];
            if !self.in_string {
                self.consume(1);
                self.output.push(byte);
                if byte == b'"' {
                    self.in_string = true;
                }
                continue;
            }

            if !self.escape {
                self.consume(1);
                self.output.push(byte);
                if byte == b'\\' {
                    self.escape = true;
                } else if byte == b'"' {
                    self.in_string = false;
                }
                continue;
            }

            let next = match self.peek(1)? {
                Some(value) => value[0],
                None => break,
            };

            if next != b'u' {
                self.consume(1);
                self.output.push(next);
                self.escape = false;
                continue;
            }

            let slice = match self.peek(5)? {
                Some(value) => value,
                None => break,
            };

            let digits = [slice[1], slice[2], slice[3], slice[4]];
            if !digits.iter().all(|b| b.is_ascii_hexdigit()) {
                self.consume(1);
                self.output.push(b'u');
                self.escape = false;
                continue;
            }

            let code_unit = parse_hex4(&digits);

            if (0xD800..=0xDBFF).contains(&code_unit) {
                let has_pair = match self.peek(6)? {
                    Some(lookahead) => {
                        if lookahead[5] != b'\\' {
                            false
                        } else {
                            match self.peek(11)? {
                                Some(full) => is_surrogate_pair(full),
                                None => {
                                    if self.eof {
                                        false
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        if self.eof {
                            false
                        } else {
                            break;
                        }
                    }
                };
                if has_pair {
                    self.consume(5);
                    self.output.extend_from_slice(b"u");
                    self.output.extend_from_slice(&digits);
                    self.escape = false;
                    continue;
                }
                self.consume(5);
                self.output.extend_from_slice(b"uFFFD");
                self.escape = false;
                continue;
            }

            if (0xDC00..=0xDFFF).contains(&code_unit) {
                self.consume(5);
                self.output.extend_from_slice(b"uFFFD");
                self.escape = false;
                continue;
            }

            self.consume(5);
            self.output.extend_from_slice(b"u");
            self.output.extend_from_slice(&digits);
            self.escape = false;
        }

        Ok(())
    }
}

impl<'a, R: Read> Read for LenientJsonReader<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.output.is_empty() {
            self.process()?;
        }

        if self.output.is_empty() {
            return Ok(0);
        }

        let n = buf.len().min(self.output.len());
        buf[..n].copy_from_slice(&self.output[..n]);
        self.output.drain(0..n);
        Ok(n)
    }
}

fn parse_hex4(bytes: &[u8]) -> u16 {
    let mut value = 0u16;
    for b in bytes {
        let digit = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => 0,
        };
        value = (value << 4) | digit as u16;
    }
    value
}

fn is_surrogate_pair(lookahead: &[u8]) -> bool {
    if lookahead.len() < 11 {
        return false;
    }
    if lookahead[5] != b'\\' || lookahead[6] != b'u' {
        return false;
    }
    let digits = &lookahead[7..11];
    if !digits.iter().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let value = parse_hex4(digits);
    (0xDC00..=0xDFFF).contains(&value)
}
