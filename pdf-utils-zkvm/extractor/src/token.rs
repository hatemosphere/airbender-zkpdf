use alloc::string::String;
use alloc::vec::Vec;
use core::str;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Number(f32),
    String(Vec<u8>),
    Name(String),
    Operator(String),
    ArrayStart,
    ArrayEnd,
    DictStart,
    DictEnd,
}

pub struct TokenParser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> TokenParser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn parse_all(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        while self.pos < self.data.len() {
            self.skip_whitespace();
            if self.pos >= self.data.len() {
                break;
            }

            if let Some(token) = self.parse_token() {
                tokens.push(token);
            }
        }
        tokens
    }

    fn parse_token(&mut self) -> Option<Token> {
        match self.peek()? {
            b'/' => self.parse_name(),
            b'(' => self.parse_string(),
            b'<' => {
                if self.peek_at(1) == Some(b'<') {
                    self.pos += 2;
                    Some(Token::DictStart)
                } else if self.peek_at(1) == Some(b'>') {
                    // Empty hex string
                    self.pos += 2;
                    Some(Token::String(Vec::new()))
                } else {
                    self.parse_hex_string()
                }
            }
            b'>' if self.peek_at(1) == Some(b'>') => {
                self.pos += 2;
                Some(Token::DictEnd)
            }
            b'[' => {
                self.pos += 1;
                Some(Token::ArrayStart)
            }
            b']' => {
                self.pos += 1;
                Some(Token::ArrayEnd)
            }
            b'-' | b'+' | b'.' | b'0'..=b'9' => self.parse_number(),
            _ => self.parse_operator(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.data.get(self.pos + offset).copied()
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn parse_name(&mut self) -> Option<Token> {
        self.pos += 1; // Skip '/'
        let start = self.pos;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_whitespace() || b"()<>[]{}/%".contains(&ch) {
                break;
            }
            self.pos += 1;
        }

        let name = str::from_utf8(&self.data[start..self.pos]).ok()?;
        Some(Token::Name(name.into()))
    }

    fn parse_string(&mut self) -> Option<Token> {
        self.pos += 1; // Skip '('
        let mut result = Vec::new();
        let mut paren_depth = 1;
        let mut escape = false;

        while paren_depth > 0 {
            let ch = self.peek()?;
            self.pos += 1;

            if escape {
                let escaped = match ch {
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'b' => b'\x08',
                    b'f' => b'\x0C',
                    b'(' => b'(',
                    b')' => b')',
                    b'\\' => b'\\',
                    b'0'..=b'7' => {
                        // Octal escape
                        let mut octal = (ch - b'0') as u32;
                        for _ in 0..2 {
                            if let Some(d @ b'0'..=b'7') = self.peek() {
                                self.pos += 1;
                                octal = octal * 8 + (d - b'0') as u32;
                            } else {
                                break;
                            }
                        }
                        octal.min(255) as u8
                    }
                    _ => ch,
                };
                result.push(escaped);
                escape = false;
            } else if ch == b'\\' {
                escape = true;
            } else if ch == b'(' {
                paren_depth += 1;
                result.push(ch);
            } else if ch == b')' {
                paren_depth -= 1;
                if paren_depth > 0 {
                    result.push(ch);
                }
            } else {
                result.push(ch);
            }
        }

        Some(Token::String(result))
    }

    fn parse_hex_string(&mut self) -> Option<Token> {
        self.pos += 1; // Skip '<'
        let mut hex_chars = Vec::new();

        while let Some(ch) = self.peek() {
            if ch == b'>' {
                self.pos += 1;
                break;
            } else if ch.is_ascii_hexdigit() {
                hex_chars.push(ch);
                self.pos += 1;
            } else if ch.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                return None;
            }
        }

        // Pad with 0 if odd
        if hex_chars.len() % 2 == 1 {
            hex_chars.push(b'0');
        }

        let mut result = Vec::new();
        for chunk in hex_chars.chunks(2) {
            let high = hex_digit_value(chunk[0])?;
            let low = hex_digit_value(chunk[1])?;
            result.push((high << 4) | low);
        }

        Some(Token::String(result))
    }

    fn parse_number(&mut self) -> Option<Token> {
        let start = self.pos;

        if self.peek() == Some(b'-') || self.peek() == Some(b'+') {
            self.pos += 1;
        }

        let mut has_dot = false;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                self.pos += 1;
            } else if ch == b'.' && !has_dot {
                has_dot = true;
                self.pos += 1;
            } else {
                break;
            }
        }

        let num_str = str::from_utf8(&self.data[start..self.pos]).ok()?;
        let num = num_str.parse::<f32>().ok()?;
        Some(Token::Number(num))
    }

    fn parse_operator(&mut self) -> Option<Token> {
        let start = self.pos;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_whitespace() || b"()<>[]{}/%".contains(&ch) {
                break;
            }
            self.pos += 1;
        }

        if self.pos > start {
            let op = str::from_utf8(&self.data[start..self.pos]).ok()?;
            Some(Token::Operator(op.into()))
        } else {
            self.pos += 1; // Skip unknown character
            None
        }
    }
}

fn hex_digit_value(ch: u8) -> Option<u8> {
    match ch {
        b'0'..=b'9' => Some(ch - b'0'),
        b'A'..=b'F' => Some(ch - b'A' + 10),
        b'a'..=b'f' => Some(ch - b'a' + 10),
        _ => None,
    }
}
