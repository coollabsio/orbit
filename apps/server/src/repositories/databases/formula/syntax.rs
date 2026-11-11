//! Lexer and parser of the formula grammar (spec 15.4). The parser caps nesting while it
//! recurses and counts nodes as it builds them, so an untrusted expression cannot overflow the
//! stack or build a large tree.

use super::{FormulaError, MAX_DEPTH, MAX_NODES, Span};

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Number(f64),
    Text(String),
    Name(String),
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Dot,
    Question,
    Colon,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    End,
}

impl Tok {
    fn describe(&self) -> String {
        match self {
            Self::Number(number) => format!("the number {number}"),
            Self::Text(_) => "a text".to_owned(),
            Self::Name(name) => format!("`{name}`"),
            Self::End => "the end of the formula".to_owned(),
            other => format!("`{}`", other.symbol()),
        }
    }

    const fn symbol(&self) -> &'static str {
        match self {
            Self::LParen => "(",
            Self::RParen => ")",
            Self::LBracket => "[",
            Self::RBracket => "]",
            Self::Comma => ",",
            Self::Dot => ".",
            Self::Question => "?",
            Self::Colon => ":",
            Self::Plus => "+",
            Self::Minus => "-",
            Self::Star => "*",
            Self::Slash => "/",
            Self::Percent => "%",
            Self::Caret => "^",
            Self::EqEq => "==",
            Self::NotEq => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::AndAnd => "&&",
            Self::OrOr => "||",
            Self::Bang => "!",
            Self::Number(_) | Self::Text(_) | Self::Name(_) | Self::End => "",
        }
    }
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    span: Span,
}

fn lex(source: &str) -> Result<Vec<Token>, FormulaError> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let start = at;
        let byte = bytes[at];
        if byte.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let two = bytes.get(at..at + 2).unwrap_or_default();
        let (tok, len) = match (byte, two) {
            (_, b"==") => (Tok::EqEq, 2),
            (_, b"!=") => (Tok::NotEq, 2),
            (_, b"<=") => (Tok::Le, 2),
            (_, b">=") => (Tok::Ge, 2),
            (_, b"&&") => (Tok::AndAnd, 2),
            (_, b"||") => (Tok::OrOr, 2),
            (b'(', _) => (Tok::LParen, 1),
            (b')', _) => (Tok::RParen, 1),
            (b'[', _) => (Tok::LBracket, 1),
            (b']', _) => (Tok::RBracket, 1),
            (b',', _) => (Tok::Comma, 1),
            (b'.', _) => (Tok::Dot, 1),
            (b'?', _) => (Tok::Question, 1),
            (b':', _) => (Tok::Colon, 1),
            (b'+', _) => (Tok::Plus, 1),
            (b'-', _) => (Tok::Minus, 1),
            (b'*', _) => (Tok::Star, 1),
            (b'/', _) => (Tok::Slash, 1),
            (b'%', _) => (Tok::Percent, 1),
            (b'^', _) => (Tok::Caret, 1),
            (b'<', _) => (Tok::Lt, 1),
            (b'>', _) => (Tok::Gt, 1),
            (b'!', _) => (Tok::Bang, 1),
            (b'"', _) => {
                let (text, end) = lex_text(source, at)?;
                tokens.push(Token {
                    tok: Tok::Text(text),
                    span: Span::new(start, end),
                });
                at = end;
                continue;
            }
            (b'0'..=b'9', _) => {
                let end = number_end(bytes, at);
                let number = source[at..end]
                    .parse::<f64>()
                    .ok()
                    .filter(|number| number.is_finite())
                    .ok_or_else(|| {
                        FormulaError::new("This number is too large.", Span::new(at, end))
                    })?;
                tokens.push(Token {
                    tok: Tok::Number(number),
                    span: Span::new(start, end),
                });
                at = end;
                continue;
            }
            (b'a'..=b'z' | b'A'..=b'Z' | b'_', _) => {
                let mut end = at;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
                {
                    end += 1;
                }
                tokens.push(Token {
                    tok: Tok::Name(source[at..end].to_owned()),
                    span: Span::new(start, end),
                });
                at = end;
                continue;
            }
            _ => {
                let len = source[at..].chars().next().map_or(1, char::len_utf8);
                return Err(FormulaError::new(
                    format!("Unexpected `{}`.", &source[at..at + len]),
                    Span::new(at, at + len),
                ));
            }
        };
        tokens.push(Token {
            tok,
            span: Span::new(start, start + len),
        });
        at += len;
    }
    tokens.push(Token {
        tok: Tok::End,
        span: Span::new(bytes.len(), bytes.len()),
    });
    Ok(tokens)
}

/// A string from its opening quote at `start`: the text and the index after the closing quote.
fn lex_text(source: &str, start: usize) -> Result<(String, usize), FormulaError> {
    let mut text = String::new();
    let mut chars = source[start + 1..].char_indices();
    while let Some((offset, char)) = chars.next() {
        let at = start + 1 + offset;
        match char {
            '"' => return Ok((text, at + 1)),
            '\\' => match chars.next() {
                Some((_, '"')) => text.push('"'),
                Some((_, '\\')) => text.push('\\'),
                Some((_, 'n')) => text.push('\n'),
                Some((_, other)) => {
                    return Err(FormulaError::new(
                        format!("Unknown escape `\\{other}`. Use \\\", \\\\ or \\n."),
                        Span::new(at, at + 1 + other.len_utf8()),
                    ));
                }
                None => break,
            },
            other => text.push(other),
        }
    }
    Err(FormulaError::new(
        "This text has no closing `\"`.",
        Span::new(start, source.len()),
    ))
}

fn number_end(bytes: &[u8], start: usize) -> usize {
    let digits = |mut at: usize| {
        while at < bytes.len() && bytes[at].is_ascii_digit() {
            at += 1;
        }
        at
    };
    let mut end = digits(start);
    if bytes.get(end) == Some(&b'.') && bytes.get(end + 1).is_some_and(u8::is_ascii_digit) {
        end = digits(end + 1);
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(bytes.get(end + 1) == Some(&b'-'));
        if bytes.get(end + 1 + sign).is_some_and(u8::is_ascii_digit) {
            end = digits(end + 1 + sign);
        }
    }
    end
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Power,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}

impl BinaryOp {
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Remainder => "%",
            Self::Power => "^",
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Less => "<",
            Self::LessEqual => "<=",
            Self::Greater => ">",
            Self::GreaterEqual => ">=",
            Self::And => "and",
            Self::Or => "or",
        }
    }
}

#[derive(Clone, Debug)]
pub enum NodeKind {
    Number(f64),
    Text(String),
    Bool(bool),
    List(Vec<Node>),
    /// A `let` variable, `current` or `index`.
    Name(String),
    /// `f(a, b)`, and `a.f(b)` as `f(a, b)`.
    Call {
        name: String,
        name_span: Span,
        args: Vec<Node>,
    },
    Negate(Box<Node>),
    Not(Box<Node>),
    Binary(BinaryOp, Box<Node>, Box<Node>),
    /// A left-to-right run of `or`, `and`, `+`/`-` or `*`/`/`/`%`: flat, so its length adds no
    /// depth and no walk recurses once per operator.
    Chain(Box<Node>, Vec<(BinaryOp, Node)>),
    Ternary(Box<Node>, Box<Node>, Box<Node>),
}

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: NodeKind,
    pub span: Span,
}

/// Parses `source`, at most [`MAX_NODES`] nodes and [`MAX_DEPTH`] levels deep.
pub fn parse(source: &str) -> Result<Node, FormulaError> {
    let tokens = lex(source)?;
    let mut parser = Parser {
        tokens,
        at: 0,
        nodes: 0,
        depth: 0,
    };
    let node = parser.expr()?;
    let next = parser.peek();
    if next.tok != Tok::End {
        return Err(FormulaError::new(
            format!(
                "Unexpected {} after the end of the formula.",
                next.tok.describe()
            ),
            next.span,
        ));
    }
    if depth(&node) > MAX_DEPTH {
        return Err(too_deep(node.span));
    }
    Ok(node)
}

fn too_deep(span: Span) -> FormulaError {
    FormulaError::new(
        format!("This formula is nested too deeply (at most {MAX_DEPTH} levels)."),
        span,
    )
}

/// The tree's depth. The tree has at most [`MAX_NODES`] nodes, so the recursion is bounded.
fn depth(node: &Node) -> usize {
    let children = match &node.kind {
        NodeKind::Number(_) | NodeKind::Text(_) | NodeKind::Bool(_) | NodeKind::Name(_) => {
            return 1;
        }
        NodeKind::List(items) | NodeKind::Call { args: items, .. } => {
            items.iter().map(depth).max().unwrap_or(0)
        }
        NodeKind::Negate(inner) | NodeKind::Not(inner) => depth(inner),
        NodeKind::Binary(_, left, right) => depth(left).max(depth(right)),
        NodeKind::Chain(first, rest) => rest
            .iter()
            .map(|(_, node)| depth(node))
            .fold(depth(first), usize::max),
        NodeKind::Ternary(test, then, other) => depth(test).max(depth(then)).max(depth(other)),
    };
    children + 1
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    nodes: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.at]
    }

    fn next(&mut self) -> Token {
        let token = self.tokens[self.at].clone();
        if token.tok != Tok::End {
            self.at += 1;
        }
        token
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if &self.peek().tok == tok {
            self.next();
            true
        } else {
            false
        }
    }

    fn eat_name(&mut self, name: &str) -> bool {
        if matches!(&self.peek().tok, Tok::Name(found) if found == name) {
            self.next();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, tok: &Tok, after: &str) -> Result<Span, FormulaError> {
        let token = self.peek().clone();
        if &token.tok == tok {
            self.next();
            return Ok(token.span);
        }
        Err(FormulaError::new(
            format!(
                "Expected `{}` {after}, found {}.",
                tok.symbol(),
                token.tok.describe()
            ),
            token.span,
        ))
    }

    fn node(&mut self, kind: NodeKind, span: Span) -> Result<Node, FormulaError> {
        self.count(span)?;
        Ok(Node { kind, span })
    }

    fn count(&mut self, span: Span) -> Result<(), FormulaError> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(FormulaError::new(
                format!("This formula is too long (at most {MAX_NODES} parts)."),
                span,
            ));
        }
        Ok(())
    }

    fn enter(&mut self) -> Result<(), FormulaError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(too_deep(self.peek().span));
        }
        Ok(())
    }

    fn expr(&mut self) -> Result<Node, FormulaError> {
        self.enter()?;
        let node = self.ternary();
        self.depth -= 1;
        node
    }

    fn ternary(&mut self) -> Result<Node, FormulaError> {
        let test = self.or()?;
        if !self.eat(&Tok::Question) {
            return Ok(test);
        }
        let then = self.expr()?;
        self.expect(&Tok::Colon, "after the `?` branch")?;
        let other = self.expr()?;
        let span = test.span.to(other.span);
        self.node(
            NodeKind::Ternary(Box::new(test), Box::new(then), Box::new(other)),
            span,
        )
    }

    /// Operands joined by the operators `op` eats, one node per operator.
    fn chain(
        &mut self,
        operand: fn(&mut Self) -> Result<Node, FormulaError>,
        op: fn(&mut Self) -> Option<BinaryOp>,
    ) -> Result<Node, FormulaError> {
        let first = operand(self)?;
        let mut rest = Vec::new();
        while let Some(op) = op(self) {
            let right = operand(self)?;
            self.count(first.span.to(right.span))?;
            rest.push((op, right));
        }
        let Some(span) = rest.last().map(|(_, last)| first.span.to(last.span)) else {
            return Ok(first);
        };
        Ok(Node {
            kind: NodeKind::Chain(Box::new(first), rest),
            span,
        })
    }

    fn or(&mut self) -> Result<Node, FormulaError> {
        self.chain(Self::and, |parser| {
            (parser.eat(&Tok::OrOr) || parser.eat_name("or")).then_some(BinaryOp::Or)
        })
    }

    fn and(&mut self) -> Result<Node, FormulaError> {
        self.chain(Self::not, |parser| {
            (parser.eat(&Tok::AndAnd) || parser.eat_name("and")).then_some(BinaryOp::And)
        })
    }

    fn not(&mut self) -> Result<Node, FormulaError> {
        let start = self.peek().span;
        if self.eat(&Tok::Bang) || self.eat_name("not") {
            self.enter()?;
            let inner = self.not();
            self.depth -= 1;
            let inner = inner?;
            let span = start.to(inner.span);
            return self.node(NodeKind::Not(Box::new(inner)), span);
        }
        self.compare()
    }

    fn compare(&mut self) -> Result<Node, FormulaError> {
        let left = self.add()?;
        let op = match self.peek().tok {
            Tok::EqEq => BinaryOp::Equal,
            Tok::NotEq => BinaryOp::NotEqual,
            Tok::Lt => BinaryOp::Less,
            Tok::Le => BinaryOp::LessEqual,
            Tok::Gt => BinaryOp::Greater,
            Tok::Ge => BinaryOp::GreaterEqual,
            _ => return Ok(left),
        };
        self.next();
        let right = self.add()?;
        let span = left.span.to(right.span);
        self.node(NodeKind::Binary(op, Box::new(left), Box::new(right)), span)
    }

    fn add(&mut self) -> Result<Node, FormulaError> {
        self.chain(Self::mul, |parser| {
            let op = match parser.peek().tok {
                Tok::Plus => BinaryOp::Add,
                Tok::Minus => BinaryOp::Subtract,
                _ => return None,
            };
            parser.next();
            Some(op)
        })
    }

    fn mul(&mut self) -> Result<Node, FormulaError> {
        self.chain(Self::unary, |parser| {
            let op = match parser.peek().tok {
                Tok::Star => BinaryOp::Multiply,
                Tok::Slash => BinaryOp::Divide,
                Tok::Percent => BinaryOp::Remainder,
                _ => return None,
            };
            parser.next();
            Some(op)
        })
    }

    fn unary(&mut self) -> Result<Node, FormulaError> {
        let start = self.peek().span;
        if self.eat(&Tok::Minus) {
            self.enter()?;
            let inner = self.unary();
            self.depth -= 1;
            let inner = inner?;
            let span = start.to(inner.span);
            return self.node(NodeKind::Negate(Box::new(inner)), span);
        }
        self.power()
    }

    fn power(&mut self) -> Result<Node, FormulaError> {
        let base = self.postfix()?;
        if !self.eat(&Tok::Caret) {
            return Ok(base);
        }
        self.enter()?;
        let exponent = self.unary();
        self.depth -= 1;
        let exponent = exponent?;
        let span = base.span.to(exponent.span);
        self.node(
            NodeKind::Binary(BinaryOp::Power, Box::new(base), Box::new(exponent)),
            span,
        )
    }

    fn postfix(&mut self) -> Result<Node, FormulaError> {
        let mut node = self.primary()?;
        while self.eat(&Tok::Dot) {
            let token = self.next();
            let Tok::Name(name) = token.tok else {
                return Err(FormulaError::new(
                    format!(
                        "Expected a function name after `.`, found {}.",
                        token.tok.describe()
                    ),
                    token.span,
                ));
            };
            self.expect(&Tok::LParen, &format!("after `.{name}`"))?;
            let mut args = vec![node];
            let end = self.args(&mut args, &name)?;
            let span = args[0].span.to(end);
            node = self.node(
                NodeKind::Call {
                    name,
                    name_span: token.span,
                    args,
                },
                span,
            )?;
        }
        Ok(node)
    }

    /// The arguments after `(` up to and with `)`; returns the span of `)`.
    fn args(&mut self, args: &mut Vec<Node>, name: &str) -> Result<Span, FormulaError> {
        let close = |parser: &mut Self| {
            parser.expect(&Tok::RParen, &format!("after the arguments of `{name}`"))
        };
        if let Some(span) = self
            .eat(&Tok::RParen)
            .then(|| self.tokens[self.at - 1].span)
        {
            return Ok(span);
        }
        loop {
            args.push(self.expr()?);
            if !self.eat(&Tok::Comma) {
                return close(self);
            }
        }
    }

    fn primary(&mut self) -> Result<Node, FormulaError> {
        let token = self.next();
        let span = token.span;
        match token.tok {
            Tok::Number(number) => self.node(NodeKind::Number(number), span),
            Tok::Text(text) => self.node(NodeKind::Text(text), span),
            Tok::LParen => {
                let inner = self.expr()?;
                let end = self.expect(&Tok::RParen, "to close `(`")?;
                Ok(Node {
                    span: span.to(end),
                    ..inner
                })
            }
            Tok::LBracket => {
                let mut items = Vec::new();
                if !self.eat(&Tok::RBracket) {
                    loop {
                        items.push(self.expr()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(&Tok::RBracket, "to close the list")?;
                }
                let end = self.tokens[self.at - 1].span;
                self.node(NodeKind::List(items), span.to(end))
            }
            Tok::Name(name) => match name.as_str() {
                "true" => self.node(NodeKind::Bool(true), span),
                "false" => self.node(NodeKind::Bool(false), span),
                _ if self.eat(&Tok::LParen) => {
                    let mut args = Vec::new();
                    let end = self.args(&mut args, &name)?;
                    self.node(
                        NodeKind::Call {
                            name,
                            name_span: span,
                            args,
                        },
                        span.to(end),
                    )
                }
                _ => self.node(NodeKind::Name(name), span),
            },
            other => Err(FormulaError::new(
                format!("Expected a value, found {}.", other.describe()),
                span,
            )),
        }
    }
}
