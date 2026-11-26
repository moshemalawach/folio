//! Equations: a small parser for school maths typed as LaTeX (`\frac{a}{b}`) or in
//! the desktop notebook's linear format (`a/b`, `x^2`), a box layout painted with egui, and MathML
//! and LaTeX output for export and copying.

use eframe::egui::{
    self, pos2, vec2, Color32, CornerRadius, FontFamily, FontId, Painter, Pos2, Rect, Shape,
    Stroke, Vec2,
};

use std::borrow::Cow;

use crate::i18n::t;
use crate::theme;
use crate::util;

/// Deeper nesting than this is refused, so pasted junk can't overflow the stack.
const MAX_DEPTH: usize = 48;

/// Function names typed as plain letters. Longer names first: the longest prefix wins.
const FUNCTIONS: [&str; 24] = [
    "arcsin", "arccos", "arctan", "sqrt", "sinh", "cosh", "tanh", "sin", "cos", "tan", "cot",
    "sec", "csc", "log", "exp", "lim", "max", "min", "det", "gcd", "sup", "inf", "mod", "ln",
];
/// Names whose sub- and superscripts go below and above, as limits.
const LIMITS: [&str; 5] = ["lim", "max", "min", "sup", "inf"];
const INTEGRALS: [char; 3] = ['∫', '∬', '∮'];
/// Commands that only size or style delimiters; delimiters size themselves here.
const IGNORED: [&str; 14] = [
    "left",
    "right",
    "big",
    "Big",
    "bigg",
    "Bigg",
    "bigl",
    "bigr",
    "Bigl",
    "Bigr",
    "displaystyle",
    "textstyle",
    "limits",
    "nolimits",
];
/// LaTeX name, symbol, italic. The first name for a symbol is the one written back.
const GREEK: [(&str, char, bool); 41] = [
    ("alpha", 'α', true),
    ("beta", 'β', true),
    ("gamma", 'γ', true),
    ("delta", 'δ', true),
    ("epsilon", 'ε', true),
    ("varepsilon", 'ε', true),
    ("zeta", 'ζ', true),
    ("eta", 'η', true),
    ("theta", 'θ', true),
    ("vartheta", 'θ', true),
    ("iota", 'ι', true),
    ("kappa", 'κ', true),
    ("lambda", 'λ', true),
    ("mu", 'μ', true),
    ("nu", 'ν', true),
    ("xi", 'ξ', true),
    ("omicron", 'ο', true),
    ("pi", 'π', true),
    ("varpi", 'π', true),
    ("rho", 'ρ', true),
    ("varrho", 'ρ', true),
    ("sigma", 'σ', true),
    ("varsigma", 'ς', true),
    ("tau", 'τ', true),
    ("upsilon", 'υ', true),
    ("phi", 'φ', true),
    ("varphi", 'φ', true),
    ("chi", 'χ', true),
    ("psi", 'ψ', true),
    ("omega", 'ω', true),
    ("Gamma", 'Γ', false),
    ("Delta", 'Δ', false),
    ("Theta", 'Θ', false),
    ("Lambda", 'Λ', false),
    ("Xi", 'Ξ', false),
    ("Pi", 'Π', false),
    ("Sigma", 'Σ', false),
    ("Upsilon", 'Υ', false),
    ("Phi", 'Φ', false),
    ("Psi", 'Ψ', false),
    ("Omega", 'Ω', false),
];
/// Upright symbols that behave like letters.
const SYMBOLS: [(&str, char); 10] = [
    ("infty", '∞'),
    ("partial", '∂'),
    ("nabla", '∇'),
    ("ldots", '…'),
    ("dots", '…'),
    ("cdots", '⋯'),
    ("degree", '°'),
    ("emptyset", '∅'),
    ("forall", '∀'),
    ("exists", '∃'),
];
const OPERATORS: [(&str, char); 35] = [
    ("times", '×'),
    ("div", '÷'),
    ("cdot", '·'),
    ("pm", '±'),
    ("mp", '∓'),
    ("le", '≤'),
    ("leq", '≤'),
    ("ge", '≥'),
    ("geq", '≥'),
    ("ne", '≠'),
    ("neq", '≠'),
    ("approx", '≈'),
    ("equiv", '≡'),
    ("sim", '∼'),
    ("to", '→'),
    ("rightarrow", '→'),
    ("leftarrow", '←'),
    ("Rightarrow", '⇒'),
    ("implies", '⇒'),
    ("Leftrightarrow", '⇔'),
    ("iff", '⇔'),
    ("in", '∈'),
    ("notin", '∉'),
    ("subset", '⊂'),
    ("subseteq", '⊆'),
    ("cup", '∪'),
    ("cap", '∩'),
    ("propto", '∝'),
    ("perp", '⊥'),
    ("parallel", '∥'),
    ("angle", '∠'),
    ("circ", '∘'),
    ("ast", '∗'),
    ("lt", '<'),
    ("gt", '>'),
];
const BIG_OPERATORS: [(&str, char); 5] = [
    ("sum", '∑'),
    ("prod", '∏'),
    ("int", '∫'),
    ("iint", '∬'),
    ("oint", '∮'),
];

/// Insert › Equation ▾: Office's ready-made equations (label id, LaTeX).
pub const READY_EQUATIONS: [(&str, &str); 9] = [
    ("equation-area-of-circle", r"A=\pi r^2"),
    (
        "equation-binomial",
        r"(x+a)^n=\sum_{k=0}^{n}\binom{n}{k}x^k a^{n-k}",
    ),
    (
        "equation-expansion-of-sum",
        r"(1+x)^n=1+\frac{nx}{1!}+\frac{n(n-1)x^2}{2!}+\dots",
    ),
    (
        "equation-fourier",
        r"f(x)=a_0+\sum_{n=1}^{\infty}\left(a_n\cos\frac{n\pi x}{L}+b_n\sin\frac{n\pi x}{L}\right)",
    ),
    ("equation-pythagorean", r"a^2+b^2=c^2"),
    ("equation-quadratic", r"x=\frac{-b\pm\sqrt{b^2-4ac}}{2a}"),
    (
        "equation-taylor",
        r"e^x=1+\frac{x}{1!}+\frac{x^2}{2!}+\frac{x^3}{3!}+\dots",
    ),
    (
        "equation-trig-1",
        r"\sin\alpha\pm\sin\beta=2\sin\frac{1}{2}(\alpha\pm\beta)\cos\frac{1}{2}(\alpha\mp\beta)",
    ),
    (
        "equation-trig-2",
        r"\cos\alpha+\cos\beta=2\cos\frac{1}{2}(\alpha+\beta)\cos\frac{1}{2}(\alpha-\beta)",
    ),
];

/// A parsed equation.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Row(Vec<Node>),
    Number(String),
    /// A variable (italic), or a function name or symbol (upright).
    Ident {
        text: String,
        italic: bool,
    },
    Text(String),
    /// A command this parser doesn't know (`\\mathbb`): shown as typed, and written back
    /// as is so the LaTeX still compiles.
    Command(String),
    Op(char),
    BigOp(char),
    Frac(Box<Node>, Box<Node>),
    /// `\binom{n}{k}`: a fraction with no bar, in parentheses.
    Binom(Box<Node>, Box<Node>),
    Scripts {
        base: Box<Node>,
        sub: Option<Box<Node>>,
        sup: Option<Box<Node>>,
    },
    Sqrt {
        index: Option<Box<Node>>,
        body: Box<Node>,
    },
    Fenced {
        open: Option<char>,
        close: Option<char>,
        body: Box<Node>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
}

fn error(message: impl Into<String>) -> ParseError {
    ParseError {
        message: message.into(),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(String),
    Func(String),
    Letter(char),
    Cmd(String),
    Char(char),
    Space,
}

/// The tokens of `chars`, each with the span of characters it was read from.
fn tokenize(chars: &[char]) -> (Vec<Tok>, Vec<(usize, usize)>) {
    let mut toks = Vec::new();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if c.is_whitespace() {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            toks.push(Tok::Space);
            spans.push((start, i));
            continue;
        }
        if c.is_ascii_digit() {
            // `3.5` and the French `3,5` are one number.
            i += 1;
            while i < chars.len() {
                let separator = matches!(chars[i], '.' | ',')
                    && chars.get(i + 1).is_some_and(char::is_ascii_digit);
                if chars[i].is_ascii_digit() || separator {
                    i += 1;
                } else {
                    break;
                }
            }
            toks.push(Tok::Num(chars[start..i].iter().collect()));
            spans.push((start, i));
            continue;
        }
        if c.is_alphabetic() {
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let mut rest = word.as_str();
            let mut at = start;
            if let Some(name) = FUNCTIONS.iter().find(|name| rest.starts_with(**name)) {
                toks.push(Tok::Func((*name).to_string()));
                spans.push((at, at + name.len()));
                at += name.len();
                rest = &rest[name.len()..];
            }
            for letter in rest.chars() {
                toks.push(Tok::Letter(letter));
                spans.push((at, at + 1));
                at += 1;
            }
            continue;
        }
        if c == '\\' {
            let name_start = i + 1;
            let mut end = name_start;
            while end < chars.len() && chars[end].is_ascii_alphabetic() {
                end += 1;
            }
            if end > name_start {
                toks.push(Tok::Cmd(chars[name_start..end].iter().collect()));
                i = end;
            } else if let Some(symbol) = chars.get(name_start) {
                // `\{`, `\,`, `\%` and friends.
                toks.push(Tok::Cmd(symbol.to_string()));
                i = name_start + 1;
            } else {
                toks.push(Tok::Char('\\'));
                i = name_start;
            }
            spans.push((start, i));
            continue;
        }
        let pair = match (c, chars.get(i + 1).copied()) {
            ('<', Some('=')) => Some('≤'),
            ('>', Some('=')) => Some('≥'),
            ('!', Some('=')) => Some('≠'),
            ('-', Some('>')) => Some('→'),
            ('=', Some('>')) => Some('⇒'),
            ('+', Some('-')) => Some('±'),
            _ => None,
        };
        if let Some(symbol) = pair {
            toks.push(Tok::Char(symbol));
            i += 2;
            spans.push((start, i));
            continue;
        }
        toks.push(Tok::Char(match c {
            '-' => '−',
            '*' => '·',
            '\'' => '′',
            other => other,
        }));
        i += 1;
        spans.push((start, i));
    }
    (toks, spans)
}

#[derive(Clone, Copy, PartialEq)]
enum Stop {
    End,
    Brace,
    Close,
    Bracket,
    Pipe,
}

struct Parser {
    /// The source, for arguments read as typed (`\text{…}`).
    chars: Vec<char>,
    toks: Vec<Tok>,
    /// Where each token is in `chars`.
    spans: Vec<(usize, usize)>,
    pos: usize,
    depth: usize,
}

/// Parses an equation. Never panics; see the module docs for what is accepted.
pub fn parse(source: &str) -> Result<Node, ParseError> {
    let chars: Vec<char> = source.chars().collect();
    let (toks, spans) = tokenize(&chars);
    let mut parser = Parser {
        chars,
        toks,
        spans,
        pos: 0,
        depth: 0,
    };
    parser.row(Stop::End)
}

/// One node for a group: its only item, or a row.
fn group(mut items: Vec<Node>) -> Node {
    if items.len() == 1 {
        items.remove(0)
    } else {
        Node::Row(items)
    }
}

/// `(a+b)/c` and `x^(n+1)` drop the parentheses, as the desktop notebook's linear format does.
fn unwrap_parens(node: Node) -> Node {
    match node {
        Node::Fenced {
            open: Some('('),
            close: Some(')'),
            body,
        } => *body,
        other => other,
    }
}

fn letter(c: char) -> Node {
    // Capital Greek letters are upright, as in print; other letters are variables.
    let italic = !('\u{391}'..='\u{3A9}').contains(&c);
    Node::Ident {
        text: c.to_string(),
        italic,
    }
}

impl Parser {
    /// The next token that isn't a space.
    fn peek(&self) -> Option<&Tok> {
        self.toks[self.pos..].iter().find(|tok| **tok != Tok::Space)
    }

    fn next(&mut self) -> Option<Tok> {
        while self.toks.get(self.pos) == Some(&Tok::Space) {
            self.pos += 1;
        }
        let tok = self.toks.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            Err(error(t!("math-error-too-deep")))
        } else {
            Ok(())
        }
    }

    fn at_stop(&self, stop: Stop) -> bool {
        match (self.peek(), stop) {
            (None, _) => true,
            (Some(Tok::Char('}')), Stop::Brace) => true,
            (Some(Tok::Char(')' | ']')), Stop::Close) => true,
            (Some(Tok::Cmd(name)), Stop::Close) => name == "}",
            (Some(Tok::Char(']')), Stop::Bracket) => true,
            (Some(Tok::Char('|')), Stop::Pipe) => true,
            _ => false,
        }
    }

    fn row(&mut self, stop: Stop) -> Result<Node, ParseError> {
        self.enter()?;
        let mut items = Vec::new();
        while !self.at_stop(stop) {
            if let Some(Tok::Cmd(name)) = self.peek().cloned() {
                if IGNORED.contains(&name.as_str()) {
                    self.next();
                    // `\left.` and `\right.` mean "no delimiter".
                    if matches!(name.as_str(), "left" | "right")
                        && self.peek() == Some(&Tok::Char('.'))
                    {
                        self.next();
                    }
                    continue;
                }
            }
            let left = self.scripted()?;
            items.push(self.fraction(left)?);
        }
        self.depth -= 1;
        Ok(group(items))
    }

    /// `a/b`, chained left to right, after the left operand was read.
    fn fraction(&mut self, mut left: Node) -> Result<Node, ParseError> {
        let depth = self.depth;
        while self.peek() == Some(&Tok::Char('/')) {
            self.next();
            // Each link of `1/2/3/…` nests the fraction so far one level deeper.
            self.enter()?;
            let right = self.scripted()?;
            left = Node::Frac(
                Box::new(unwrap_parens(left)),
                Box::new(unwrap_parens(right)),
            );
        }
        self.depth = depth;
        Ok(left)
    }

    /// An operand followed by any `_` and `^` scripts.
    fn scripted(&mut self) -> Result<Node, ParseError> {
        let depth = self.depth;
        let mut base = self.operand()?;
        loop {
            let is_sub = match self.peek() {
                Some(Tok::Char('_')) => true,
                Some(Tok::Char('^')) => false,
                _ => break,
            };
            self.next();
            // `x^1^1^…`: a script whose slot is taken nests the base one level deeper.
            let fills_slot = match &base {
                Node::Scripts { sub, sup, .. } => {
                    if is_sub {
                        sub.is_none()
                    } else {
                        sup.is_none()
                    }
                }
                _ => false,
            };
            if !fills_slot {
                self.enter()?;
            }
            let script = Box::new(self.script_operand()?);
            base = match base {
                Node::Scripts {
                    base: inner,
                    sub: None,
                    sup,
                } if is_sub => Node::Scripts {
                    base: inner,
                    sub: Some(script),
                    sup,
                },
                Node::Scripts {
                    base: inner,
                    sub,
                    sup: None,
                } if !is_sub => Node::Scripts {
                    base: inner,
                    sub,
                    sup: Some(script),
                },
                other => {
                    let (sub, sup) = if is_sub {
                        (Some(script), None)
                    } else {
                        (None, Some(script))
                    };
                    Node::Scripts {
                        base: Box::new(other),
                        sub,
                        sup,
                    }
                }
            };
        }
        self.depth = depth;
        Ok(base)
    }

    /// What follows `^` or `_`: one token or a group; `x^-1` keeps its sign.
    fn script_operand(&mut self) -> Result<Node, ParseError> {
        if let Some(Tok::Char(sign @ ('−' | '+' | '±'))) = self.peek().cloned() {
            self.next();
            let value = self.operand()?;
            return Ok(Node::Row(vec![Node::Op(sign), value]));
        }
        Ok(unwrap_parens(self.operand()?))
    }

    fn operand(&mut self) -> Result<Node, ParseError> {
        self.enter()?;
        let node = self.operand_inner()?;
        self.depth -= 1;
        Ok(node)
    }

    fn operand_inner(&mut self) -> Result<Node, ParseError> {
        let Some(tok) = self.next() else {
            return Err(error(t!("math-error-missing-end")));
        };
        Ok(match tok {
            Tok::Num(digits) => Node::Number(digits),
            Tok::Letter(c) => letter(c),
            Tok::Func(name) if name == "sqrt" => {
                let body = self.operand()?;
                Node::Sqrt {
                    index: None,
                    body: Box::new(unwrap_parens(body)),
                }
            }
            Tok::Func(name) => Node::Ident {
                text: name,
                italic: false,
            },
            Tok::Cmd(name) => return self.command(&name),
            Tok::Space => Node::Row(Vec::new()),
            Tok::Char(c) => match c {
                '{' => {
                    let body = self.row(Stop::Brace)?;
                    self.expect('}')?;
                    body
                }
                '(' | '[' => self.fenced(c, Stop::Close)?,
                '|' => self.fenced('|', Stop::Pipe)?,
                '√' | '∛' | '∜' => {
                    let index = match c {
                        '∛' => Some(Box::new(Node::Number("3".into()))),
                        '∜' => Some(Box::new(Node::Number("4".into()))),
                        _ => None,
                    };
                    let body = self.operand()?;
                    Node::Sqrt {
                        index,
                        body: Box::new(unwrap_parens(body)),
                    }
                }
                '∑' | '∏' | '∫' | '∬' | '∮' => Node::BigOp(c),
                '∞' | '∂' | '∇' | '°' | '…' | '⋯' | '∅' => Node::Ident {
                    text: c.to_string(),
                    italic: false,
                },
                '}' => return Err(error(t!("math-error-stray-close"))),
                '^' | '_' | '/' => {
                    return Err(error(t!(
                        "math-error-nothing-before",
                        symbol = c.to_string()
                    )))
                }
                other => Node::Op(other),
            },
        })
    }

    fn expect(&mut self, close: char) -> Result<(), ParseError> {
        match self.next() {
            Some(Tok::Char(c)) if c == close => Ok(()),
            _ => Err(error(t!(
                "math-error-missing-close",
                symbol = close.to_string()
            ))),
        }
    }

    /// An opening delimiter's contents and its closer, if there is one.
    fn fenced(&mut self, open: char, stop: Stop) -> Result<Node, ParseError> {
        let body = self.row(stop)?;
        let close = match self.peek().cloned() {
            Some(Tok::Char(c @ (')' | ']' | '|'))) => {
                self.next();
                Some(c)
            }
            Some(Tok::Cmd(name)) if name == "}" => {
                self.next();
                Some('}')
            }
            _ => None,
        };
        Ok(Node::Fenced {
            open: Some(open),
            close,
            body: Box::new(body),
        })
    }

    /// A `{…}` argument, or a single operand as in `\frac12`.
    fn braced(&mut self, command: &str) -> Result<Node, ParseError> {
        match self.peek().cloned() {
            Some(Tok::Char('{')) => {
                self.next();
                let body = self.row(Stop::Brace)?;
                self.expect('}')?;
                Ok(body)
            }
            None | Some(Tok::Char('}')) => Err(error(t!(
                "math-error-needs-group",
                command = format!("\\{command}")
            ))),
            _ => self.operand(),
        }
    }

    /// A `{…}` argument exactly as typed (spaces, apostrophes and hyphens included),
    /// not as maths tokens.
    fn raw_text(&mut self, command: &str) -> Result<String, ParseError> {
        while self.toks.get(self.pos) == Some(&Tok::Space) {
            self.pos += 1;
        }
        if self.toks.get(self.pos) != Some(&Tok::Char('{')) {
            return Err(error(t!(
                "math-error-needs-group",
                command = format!("\\{command}")
            )));
        }
        let start = self.spans[self.pos].1;
        self.pos += 1;
        let mut depth = 0usize;
        while let Some(tok) = self.toks.get(self.pos) {
            let (end, _) = self.spans[self.pos];
            self.pos += 1;
            match tok {
                Tok::Char('}') if depth == 0 => {
                    return Ok(self.chars[start..end].iter().collect());
                }
                Tok::Char('}') => depth -= 1,
                Tok::Char('{') => depth += 1,
                _ => {}
            }
        }
        Err(error(t!("math-error-missing-close", symbol = "}")))
    }

    fn command(&mut self, name: &str) -> Result<Node, ParseError> {
        if let Some(&(_, symbol, italic)) = GREEK.iter().find(|(n, ..)| *n == name) {
            return Ok(Node::Ident {
                text: symbol.to_string(),
                italic,
            });
        }
        if let Some(&(_, symbol)) = SYMBOLS.iter().find(|(n, _)| *n == name) {
            return Ok(Node::Ident {
                text: symbol.to_string(),
                italic: false,
            });
        }
        if let Some(&(_, symbol)) = OPERATORS.iter().find(|(n, _)| *n == name) {
            return Ok(Node::Op(symbol));
        }
        if let Some(&(_, symbol)) = BIG_OPERATORS.iter().find(|(n, _)| *n == name) {
            return Ok(Node::BigOp(symbol));
        }
        if name != "sqrt" && FUNCTIONS.contains(&name) {
            return Ok(Node::Ident {
                text: name.to_string(),
                italic: false,
            });
        }
        match name {
            "frac" | "dfrac" | "tfrac" => {
                let num = self.braced(name)?;
                let den = self.braced(name)?;
                Ok(Node::Frac(Box::new(num), Box::new(den)))
            }
            "binom" | "dbinom" | "tbinom" => {
                let top = self.braced(name)?;
                let bottom = self.braced(name)?;
                Ok(Node::Binom(Box::new(top), Box::new(bottom)))
            }
            "sqrt" => {
                let index = if self.peek() == Some(&Tok::Char('[')) {
                    self.next();
                    let index = self.row(Stop::Bracket)?;
                    self.expect(']')?;
                    Some(Box::new(index))
                } else {
                    None
                };
                let body = self.braced(name)?;
                Ok(Node::Sqrt {
                    index,
                    body: Box::new(unwrap_parens(body)),
                })
            }
            "operatorname" => Ok(Node::Ident {
                text: self.raw_text(name)?,
                italic: false,
            }),
            "text" | "textrm" | "mathrm" | "mbox" => Ok(Node::Text(self.raw_text(name)?)),
            "{" => self.fenced('{', Stop::Close),
            "|" => Ok(Node::Op('‖')),
            "," | ";" | ":" | " " | "quad" | "qquad" => Ok(Node::Text(" ".into())),
            "!" | "\\" => Ok(Node::Row(Vec::new())),
            "%" | "$" | "#" | "&" | "_" | "}" => Ok(Node::Op(name.chars().next().unwrap_or('?'))),
            _ => Ok(Node::Command(name.to_string())),
        }
    }
}

/// Whether scripts on `base` go below and above it (∑, lim) rather than to its right.
fn is_limits(base: &Node) -> bool {
    match base {
        Node::BigOp(c) => !INTEGRALS.contains(c),
        Node::Ident {
            text,
            italic: false,
        } => LIMITS.contains(&text.as_str()),
        _ => false,
    }
}

/// A page's text as its previews show it (the Recycle Bin, search results, the Versions
/// pane): each equation typed in LaTeX reads as the desktop notebook's linear format, `1/2` for
/// `\frac{1}{2}` and `√x` for `\sqrt{x}`, never as its source. Other lines, prose and
/// equations already linear, are kept as they are.
pub fn readable_text(text: &str) -> Cow<'_, str> {
    if !text.contains('\\') {
        return Cow::Borrowed(text);
    }
    let lines: Vec<Cow<'_, str>> = text.lines().map(readable_line).collect();
    Cow::Owned(lines.join("\n"))
}

/// One line of [`readable_text`]: an equation's LaTeX source in linear format.
fn readable_line(line: &str) -> Cow<'_, str> {
    if !looks_like_latex(line) {
        return Cow::Borrowed(line);
    }
    match parse(line) {
        Ok(node) if !has_unknown_command(&node) => {
            let mut out = String::new();
            linear(&node, &mut out);
            Cow::Owned(out.split_whitespace().collect::<Vec<_>>().join(" "))
        }
        _ => Cow::Borrowed(line),
    }
}

/// Whether `line` reads as an equation's LaTeX: it holds a command (`\frac`) and, out of
/// commands and their text arguments, no word of prose (three letters or more that are
/// not a function name such as `sin`).
fn looks_like_latex(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    let mut command = false;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && chars[end].is_ascii_alphabetic() {
                end += 1;
            }
            let name: String = chars[start..end].iter().collect();
            command |= !name.is_empty();
            i = end.max(start + 1);
            // `\text{…}` holds words: they are not prose around the equation.
            let texts = ["text", "textrm", "mathrm", "mbox", "operatorname"];
            if texts.contains(&name.as_str()) && chars.get(end) == Some(&'{') {
                let mut depth = 0;
                i = end;
                while i < chars.len() {
                    match chars[i] {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
        } else if chars[i].is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if word.len() >= 3 && !FUNCTIONS.contains(&word.as_str()) {
                return false;
            }
        } else {
            i += 1;
        }
    }
    command
}

/// Whether `node` holds a command this parser doesn't know, which it shows as typed.
fn has_unknown_command(node: &Node) -> bool {
    match node {
        Node::Command(_) => true,
        Node::Row(items) => items.iter().any(has_unknown_command),
        Node::Frac(a, b) | Node::Binom(a, b) => has_unknown_command(a) || has_unknown_command(b),
        Node::Scripts { base, sub, sup } => {
            has_unknown_command(base)
                || sub.as_deref().is_some_and(has_unknown_command)
                || sup.as_deref().is_some_and(has_unknown_command)
        }
        Node::Sqrt { index, body } => {
            index.as_deref().is_some_and(has_unknown_command) || has_unknown_command(body)
        }
        Node::Fenced { body, .. } => has_unknown_command(body),
        Node::Number(_) | Node::Ident { .. } | Node::Text(_) | Node::Op(_) | Node::BigOp(_) => {
            false
        }
    }
}

/// `node` in the desktop notebook's linear format, as text: `(a+b)/c`, `x^(n+1)`, `√(b^2−4ac)`.
fn linear(node: &Node, out: &mut String) {
    match node {
        Node::Row(items) => items.iter().for_each(|item| linear(item, out)),
        Node::Number(text) | Node::Text(text) | Node::Command(text) => out.push_str(text),
        Node::Ident { text, .. } => {
            out.push_str(text);
            // A function name keeps a space after it: sin x, not sinx.
            if text.chars().count() > 1 {
                out.push(' ');
            }
        }
        // Relations are spaced out, as the layout spaces them: `x = 1`, `a ≤ b`.
        Node::Op(c) if RELATIONS.contains(c) => {
            out.push(' ');
            out.push(*c);
            out.push(' ');
        }
        Node::Op(c) | Node::BigOp(c) => out.push(*c),
        Node::Frac(num, den) => {
            linear_operand(num, out);
            out.push('/');
            linear_operand(den, out);
        }
        Node::Binom(top, bottom) => {
            out.push('(');
            linear(top, out);
            out.push('¦');
            linear(bottom, out);
            out.push(')');
        }
        Node::Scripts { base, sub, sup } => {
            linear_operand(base, out);
            if let Some(sub) = sub {
                out.push('_');
                linear_operand(sub, out);
            }
            if let Some(sup) = sup {
                out.push('^');
                linear_operand(sup, out);
            }
        }
        Node::Sqrt { index, body } => {
            match index.as_deref() {
                None => out.push('√'),
                Some(Node::Number(n)) if n == "3" => out.push('∛'),
                Some(Node::Number(n)) if n == "4" => out.push('∜'),
                Some(index) => {
                    out.push_str("√(");
                    linear(index, out);
                    out.push('&');
                    linear(body, out);
                    out.push(')');
                    return;
                }
            }
            linear_operand(body, out);
        }
        Node::Fenced { open, close, body } => {
            out.extend(*open);
            linear(body, out);
            out.extend(*close);
        }
    }
}

/// `node` as what `/`, `^`, `_` or `√` applies to: in parentheses unless it is one piece.
fn linear_operand(node: &Node, out: &mut String) {
    let piece = match node {
        Node::Number(_) | Node::Fenced { .. } | Node::Sqrt { .. } | Node::BigOp(_) => true,
        Node::Ident { text, .. } => text.chars().count() == 1,
        _ => false,
    };
    if piece {
        linear(node, out);
    } else {
        out.push('(');
        linear(node, out);
        out.push(')');
    }
}

/// The equation as presentation MathML, for HTML export and printing.
pub fn to_mathml(node: &Node) -> String {
    format!("<math display=\"block\">{}</math>", mathml(node))
}

/// MathML for an equation's source; text that doesn't parse is shown as typed.
pub fn mathml_of(source: &str) -> String {
    match parse(source) {
        Ok(node) => to_mathml(&node),
        Err(_) => format!(
            "<math display=\"block\"><mtext>{}</mtext></math>",
            util::html_escape(source)
        ),
    }
}

fn mathml(node: &Node) -> String {
    let esc = util::html_escape;
    match node {
        Node::Row(items) if items.len() == 1 => mathml(&items[0]),
        Node::Row(items) => format!(
            "<mrow>{}</mrow>",
            items.iter().map(mathml).collect::<String>()
        ),
        Node::Number(digits) => format!("<mn>{}</mn>", esc(digits)),
        Node::Ident { text, italic } => {
            if !italic && text.chars().count() == 1 {
                format!("<mi mathvariant=\"normal\">{}</mi>", esc(text))
            } else {
                format!("<mi>{}</mi>", esc(text))
            }
        }
        Node::Text(text) => format!("<mtext>{}</mtext>", esc(text)),
        Node::Command(name) => format!("<mtext>\\{}</mtext>", esc(name)),
        Node::Op(c) => format!("<mo>{}</mo>", esc(&c.to_string())),
        Node::BigOp(c) => format!("<mo largeop=\"true\">{c}</mo>"),
        Node::Frac(num, den) => format!("<mfrac>{}{}</mfrac>", mathml(num), mathml(den)),
        Node::Binom(top, bottom) => format!(
            "<mrow><mo>(</mo><mfrac linethickness=\"0\">{}{}</mfrac><mo>)</mo></mrow>",
            mathml(top),
            mathml(bottom)
        ),
        Node::Scripts { base, sub, sup } => {
            let b = mathml(base);
            let lo = sub.as_deref().map(mathml);
            let hi = sup.as_deref().map(mathml);
            match (lo, hi, is_limits(base)) {
                (Some(lo), Some(hi), true) => format!("<munderover>{b}{lo}{hi}</munderover>"),
                (Some(lo), None, true) => format!("<munder>{b}{lo}</munder>"),
                (None, Some(hi), true) => format!("<mover>{b}{hi}</mover>"),
                (Some(lo), Some(hi), false) => format!("<msubsup>{b}{lo}{hi}</msubsup>"),
                (Some(lo), None, false) => format!("<msub>{b}{lo}</msub>"),
                (None, Some(hi), false) => format!("<msup>{b}{hi}</msup>"),
                (None, None, _) => b,
            }
        }
        Node::Sqrt { index: None, body } => format!("<msqrt>{}</msqrt>", mathml(body)),
        Node::Sqrt {
            index: Some(index),
            body,
        } => {
            format!("<mroot>{}{}</mroot>", mathml(body), mathml(index))
        }
        Node::Fenced { open, close, body } => {
            let fence = |c: &Option<char>| {
                c.map(|c| format!("<mo>{}</mo>", esc(&c.to_string())))
                    .unwrap_or_default()
            };
            format!(
                "<mrow>{}{}{}</mrow>",
                fence(open),
                mathml(body),
                fence(close)
            )
        }
    }
}

/// LaTeX for an equation's source (for "Copy LaTeX"); text that doesn't parse is kept.
pub fn latex_of(source: &str) -> String {
    parse(source).map_or_else(|_| source.to_string(), |node| to_latex(&node))
}

pub fn to_latex(node: &Node) -> String {
    let mut out = String::new();
    latex(node, &mut out);
    out.trim_end().to_string()
}

/// `\name ` — the trailing space keeps a following letter out of the command name.
fn push_command(out: &mut String, name: &str) {
    out.push('\\');
    out.push_str(name);
    out.push(' ');
}

fn latex(node: &Node, out: &mut String) {
    match node {
        Node::Row(items) => {
            for item in items {
                // A group inside a row came from `{…}`: keep its braces, so it stays the
                // argument of a command before it (`\\vec{AB}`).
                if matches!(item, Node::Row(_)) {
                    out.push('{');
                    latex(item, out);
                    out.push('}');
                } else {
                    latex(item, out);
                }
            }
        }
        Node::Number(digits) => out.push_str(digits),
        Node::Ident { text, .. } => {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => {
                    let name = GREEK
                        .iter()
                        .find(|(_, symbol, _)| *symbol == c)
                        .map(|(name, ..)| *name)
                        .or_else(|| SYMBOLS.iter().find(|(_, s)| *s == c).map(|(name, _)| *name));
                    match name {
                        Some(name) => push_command(out, name),
                        None => out.push(c),
                    }
                }
                _ if FUNCTIONS.contains(&text.as_str()) => push_command(out, text),
                _ => {
                    out.push_str("\\operatorname{");
                    out.push_str(text);
                    out.push('}');
                }
            }
        }
        Node::Text(text) => {
            out.push_str("\\text{");
            out.push_str(text);
            out.push('}');
        }
        Node::Command(name) => push_command(out, name),
        // `<` and `>` stay plain: `\lt`/`\gt` are not standard LaTeX.
        Node::Op(c @ ('<' | '>')) => out.push(*c),
        Node::Op(c) => match OPERATORS.iter().find(|(_, symbol)| symbol == c) {
            Some((name, _)) => push_command(out, name),
            None => match c {
                '−' => out.push('-'),
                '′' => out.push('\''),
                '‖' => out.push_str("\\|"),
                '%' | '$' | '#' | '&' | '_' | '{' | '}' => {
                    out.push('\\');
                    out.push(*c);
                }
                other => out.push(*other),
            },
        },
        Node::BigOp(c) => {
            let name = BIG_OPERATORS
                .iter()
                .find(|(_, symbol)| symbol == c)
                .map_or("sum", |(name, _)| name);
            push_command(out, name);
        }
        Node::Frac(num, den) => {
            out.push_str("\\frac{");
            latex(num, out);
            out.push_str("}{");
            latex(den, out);
            out.push('}');
        }
        Node::Binom(top, bottom) => {
            out.push_str("\\binom{");
            latex(top, out);
            out.push_str("}{");
            latex(bottom, out);
            out.push('}');
        }
        Node::Scripts { base, sub, sup } => {
            let braced = matches!(
                **base,
                Node::Row(_) | Node::Frac(..) | Node::Binom(..) | Node::Scripts { .. }
            );
            if braced {
                out.push('{');
            }
            latex(base, out);
            if braced {
                out.push('}');
            }
            if let Some(sub) = sub {
                out.push_str("_{");
                latex(sub, out);
                out.push('}');
            }
            if let Some(sup) = sup {
                out.push_str("^{");
                latex(sup, out);
                out.push('}');
            }
        }
        Node::Sqrt { index, body } => {
            out.push_str("\\sqrt");
            if let Some(index) = index {
                out.push('[');
                latex(index, out);
                out.push(']');
            }
            out.push('{');
            latex(body, out);
            out.push('}');
        }
        Node::Fenced { open, close, body } => {
            let delimiter = |c: char| match c {
                '{' => "\\{".to_string(),
                '}' => "\\}".to_string(),
                other => other.to_string(),
            };
            // `\\left` and `\\right` come in pairs; `.` is the invisible delimiter.
            out.push_str("\\left");
            out.push_str(&open.map_or(".".to_string(), delimiter));
            latex(body, out);
            out.push_str("\\right");
            out.push_str(&close.map_or(".".to_string(), delimiter));
        }
    }
}

// ---------------------------------------------------------------------------------------
// Layout and painting

const MIN_SIZE: f32 = 7.0;
const BINARY: [char; 7] = ['+', '−', '×', '÷', '·', '±', '∓'];
const RELATIONS: [char; 21] = [
    '=', '<', '>', '≤', '≥', '≠', '≈', '≡', '∼', '→', '←', '⇒', '⇔', '∈', '∉', '⊂', '⊆', '∝', '⊥',
    '∥', ':',
];

fn script_size(size: f32) -> f32 {
    (size * 0.7).max(MIN_SIZE)
}

#[derive(Debug, Clone)]
enum Piece {
    /// Text whose baseline starts at `at`.
    Text {
        at: Vec2,
        text: String,
        font: FontId,
    },
    Rule(Rect),
    Path {
        points: Vec<Pos2>,
        width: f32,
    },
}

impl Piece {
    fn offset(self, by: Vec2) -> Piece {
        match self {
            Piece::Text { at, text, font } => Piece::Text {
                at: at + by,
                text,
                font,
            },
            Piece::Rule(rect) => Piece::Rule(rect.translate(by)),
            Piece::Path { points, width } => Piece::Path {
                points: points.into_iter().map(|point| point + by).collect(),
                width,
            },
        }
    }
}

#[derive(Debug, Clone, Default)]
struct MathBox {
    width: f32,
    ascent: f32,
    descent: f32,
    pieces: Vec<Piece>,
}

impl MathBox {
    fn height(&self) -> f32 {
        self.ascent + self.descent
    }

    /// Adds `other`'s drawing, with its baseline-left corner moved to (`dx`, `dy`).
    fn place(&mut self, other: MathBox, dx: f32, dy: f32) {
        let by = vec2(dx, dy);
        self.pieces
            .extend(other.pieces.into_iter().map(|piece| piece.offset(by)));
    }
}

/// A typeset equation, ready to paint.
#[derive(Debug, Clone)]
pub struct MathLayout {
    /// Width and height in page units at the size it was laid out for.
    pub size: Vec2,
    /// Why the source couldn't be read; it is then shown as typed.
    pub error: Option<String>,
    ascent: f32,
    pieces: Vec<Piece>,
}

impl MathLayout {
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }
}

/// Parses and lays out `source` at `size`. Blank source gives an empty layout.
pub fn layout_source(ctx: &egui::Context, source: &str, size: f32) -> MathLayout {
    if source.trim().is_empty() {
        return MathLayout {
            size: Vec2::ZERO,
            error: None,
            ascent: 0.0,
            pieces: Vec::new(),
        };
    }
    let typesetter = Typesetter { ctx };
    let (b, error) = match parse(source) {
        Ok(node) => (typesetter.node(&node, size), None),
        Err(error) => (
            typesetter.text(source.trim(), size, false),
            Some(error.message),
        ),
    };
    MathLayout {
        size: vec2(b.width, b.height()),
        error,
        ascent: b.ascent,
        pieces: b.pieces,
    }
}

/// Paints `layout` with its top-left corner at `top_left`, scaled by `scale`.
pub fn paint(painter: &Painter, layout: &MathLayout, top_left: Pos2, scale: f32, color: Color32) {
    let origin = top_left + vec2(0.0, layout.ascent * scale);
    for piece in &layout.pieces {
        match piece {
            Piece::Text { at, text, font } => {
                let font = FontId::new(font.size * scale, font.family.clone());
                let galley = painter.layout_no_wrap(text.clone(), font, color);
                let pos = origin + *at * scale - vec2(0.0, galley_baseline(&galley));
                painter.galley(pos, galley, color);
            }
            Piece::Rule(rect) => {
                let rect = Rect::from_min_max(
                    origin + rect.min.to_vec2() * scale,
                    origin + rect.max.to_vec2() * scale,
                );
                painter.rect_filled(rect, CornerRadius::ZERO, color);
            }
            Piece::Path { points, width } => {
                let points = points
                    .iter()
                    .map(|point| origin + point.to_vec2() * scale)
                    .collect();
                painter.add(Shape::line(points, Stroke::new(width * scale, color)));
            }
        }
    }
}

/// Where the first line's baseline is, from the galley's top.
fn galley_baseline(galley: &egui::Galley) -> f32 {
    galley
        .rows
        .first()
        .and_then(|row| row.row.glyphs.first().map(|glyph| row.pos.y + glyph.pos.y))
        .unwrap_or(galley.size().y * 0.8)
}

/// The inked part of a galley: (baseline, ink top, ink bottom), from the galley's top.
fn ink_extent(galley: &egui::Galley, size: f32) -> (f32, f32, f32) {
    let baseline = galley_baseline(galley);
    let (mut top, mut bottom) = (f32::INFINITY, f32::NEG_INFINITY);
    for row in &galley.rows {
        for glyph in &row.row.glyphs {
            if glyph.uv_rect.is_nothing() {
                continue;
            }
            let y = row.pos.y + glyph.pos.y + glyph.uv_rect.offset.y;
            top = top.min(y);
            bottom = bottom.max(y + glyph.uv_rect.size.y);
        }
    }
    if top > bottom {
        // Only spaces: give the box some height so rows don't collapse.
        (baseline, baseline - size * 0.5, baseline)
    } else {
        (baseline, top, bottom)
    }
}

/// Space before and after a row item, in ems.
fn spacing(previous: Option<&Node>, node: &Node) -> (f32, f32) {
    match node {
        Node::Op(c) if RELATIONS.contains(c) => (0.28, 0.28),
        Node::Op(c) if BINARY.contains(c) => {
            // A sign at the start, or after another operator, is unary: no space.
            let unary = match previous {
                None | Some(Node::BigOp(_)) => true,
                Some(Node::Op(p)) => !matches!(p, ')' | ']' | '!' | '′'),
                _ => false,
            };
            if unary {
                (0.0, 0.0)
            } else {
                (0.22, 0.22)
            }
        }
        Node::Op(',' | ';') => (0.0, 0.18),
        Node::BigOp(_) => (0.06, 0.12),
        Node::Scripts { base, .. } if matches!(**base, Node::BigOp(_)) => (0.06, 0.12),
        Node::Ident {
            text,
            italic: false,
        } if text.chars().count() > 1 => (0.06, 0.12),
        _ => (0.0, 0.0),
    }
}

struct Typesetter<'a> {
    ctx: &'a egui::Context,
}

impl Typesetter<'_> {
    fn text(&self, text: &str, size: f32, italic: bool) -> MathBox {
        let preferred = if italic {
            theme::serif_italic(size)
        } else {
            theme::serif(size)
        };
        // Symbols the serif face lacks (∇, ⇒, ⊂ …) come from the monospace fonts.
        let font = if self
            .ctx
            .fonts_mut(|fonts| fonts.has_glyphs(&preferred, text))
        {
            preferred
        } else {
            FontId::new(size, FontFamily::Monospace)
        };
        let galley = self.ctx.fonts_mut(|fonts| {
            fonts.layout_no_wrap(text.to_owned(), font.clone(), Color32::PLACEHOLDER)
        });
        let (baseline, top, bottom) = ink_extent(&galley, size);
        MathBox {
            width: galley.size().x,
            ascent: baseline - top,
            descent: bottom - baseline,
            pieces: vec![Piece::Text {
                at: Vec2::ZERO,
                text: text.to_owned(),
                font,
            }],
        }
    }

    /// Height of the maths axis (the middle of a minus sign) above the baseline.
    fn axis(&self, size: f32) -> f32 {
        let minus = self.text("−", size, false);
        let axis = (minus.ascent - minus.descent) / 2.0;
        if axis > 0.0 && axis < size {
            axis
        } else {
            size * 0.25
        }
    }

    fn node(&self, node: &Node, size: f32) -> MathBox {
        match node {
            Node::Row(items) => self.row(items, size),
            Node::Number(digits) => self.text(digits, size, false),
            Node::Ident { text, italic } => self.text(text, size, *italic),
            Node::Text(text) => self.text(text, size, false),
            Node::Command(name) => self.text(&format!("\\{name}"), size, false),
            Node::Op(c) => self.text(&c.to_string(), size, false),
            Node::BigOp(c) => self.big_op(*c, size),
            Node::Frac(num, den) => self.fraction(num, den, size),
            Node::Binom(top, bottom) => {
                let stack = self.fraction_with(top, bottom, size, false);
                self.fence_box(Some('('), Some(')'), stack, size)
            }
            Node::Scripts { base, sub, sup } => {
                self.scripts(base, sub.as_deref(), sup.as_deref(), size)
            }
            Node::Sqrt { index, body } => self.radical(index.as_deref(), body, size),
            Node::Fenced { open, close, body } => self.fenced(*open, *close, body, size),
        }
    }

    fn row(&self, items: &[Node], size: f32) -> MathBox {
        let mut out = MathBox::default();
        if items.is_empty() {
            // An empty group (`x^{}`) still has a line's height.
            out.ascent = size * 0.7;
            out.descent = size * 0.2;
            return out;
        }
        let mut x = 0.0;
        let mut previous = None;
        for item in items {
            let (before, after) = spacing(previous, item);
            let child = self.node(item, size);
            x += before * size;
            out.ascent = out.ascent.max(child.ascent);
            out.descent = out.descent.max(child.descent);
            let width = child.width;
            out.place(child, x, 0.0);
            x += width + after * size;
            previous = Some(item);
        }
        out.width = x;
        out
    }

    fn big_op(&self, symbol: char, size: f32) -> MathBox {
        let scale = if INTEGRALS.contains(&symbol) {
            1.8
        } else {
            1.45
        };
        let glyph = self.text(&symbol.to_string(), size * scale, false);
        // Centre the symbol on the maths axis.
        let dy = (glyph.ascent - glyph.descent) / 2.0 - self.axis(size);
        let mut out = MathBox {
            width: glyph.width,
            ascent: glyph.ascent - dy,
            descent: glyph.descent + dy,
            pieces: Vec::new(),
        };
        out.place(glyph, 0.0, dy);
        out
    }

    fn fraction(&self, num: &Node, den: &Node, size: f32) -> MathBox {
        self.fraction_with(num, den, size, true)
    }

    /// `num` over `den`, with the fraction bar, or without it for `\binom`.
    fn fraction_with(&self, num: &Node, den: &Node, size: f32, bar: bool) -> MathBox {
        // Fractions inside fractions get a little smaller.
        let inner = (size * 0.9).max(MIN_SIZE);
        let num = self.node(num, inner);
        let den = self.node(den, inner);
        let rule = (size * 0.06).max(1.0);
        let gap = size * 0.14;
        let axis = self.axis(size);
        // With no bar to overhang, the parentheses around it hold it close.
        let pad = if bar { size * 0.24 } else { size * 0.08 };
        let width = num.width.max(den.width) + pad;
        let num_shift = axis + rule / 2.0 + gap + num.descent;
        let den_shift = den.ascent + gap + rule / 2.0 - axis;
        let pieces = if bar {
            vec![Piece::Rule(Rect::from_min_max(
                pos2(size * 0.06, -axis - rule / 2.0),
                pos2(width - size * 0.06, -axis + rule / 2.0),
            ))]
        } else {
            Vec::new()
        };
        let mut out = MathBox {
            width,
            ascent: num_shift + num.ascent,
            descent: den_shift + den.descent,
            pieces,
        };
        let (num_width, den_width) = (num.width, den.width);
        out.place(num, (width - num_width) / 2.0, -num_shift);
        out.place(den, (width - den_width) / 2.0, den_shift);
        out
    }

    fn scripts(&self, base: &Node, sub: Option<&Node>, sup: Option<&Node>, size: f32) -> MathBox {
        let small = script_size(size);
        let base_box = self.node(base, size);
        let sub_box = sub.map(|node| self.node(node, small));
        let sup_box = sup.map(|node| self.node(node, small));
        if is_limits(base) {
            return self.limits(base_box, sub_box, sup_box, size);
        }
        let italic = matches!(base, Node::Ident { italic: true, .. });
        let x = base_box.width + if italic { 0.05 * size } else { 0.03 * size };
        let up = sup_box.as_ref().map_or(0.0, |sup| {
            (0.45 * size).max(base_box.ascent - 0.5 * sup.ascent)
        });
        let mut down = sub_box.as_ref().map_or(0.0, |_| {
            (0.22 * size).max(base_box.descent * 0.8 + 0.1 * size)
        });
        if let (Some(sup), Some(sub)) = (&sup_box, &sub_box) {
            // Keep the superscript and subscript apart.
            let gap = (up - sup.descent) - (sub.ascent - down);
            if gap < 0.12 * size {
                down += 0.12 * size - gap;
            }
        }
        let mut out = MathBox {
            width: x,
            ascent: base_box.ascent,
            descent: base_box.descent,
            pieces: Vec::new(),
        };
        out.place(base_box, 0.0, 0.0);
        if let Some(sup) = sup_box {
            out.ascent = out.ascent.max(up + sup.ascent);
            out.descent = out.descent.max(sup.descent - up);
            out.width = out.width.max(x + sup.width);
            out.place(sup, x, -up);
        }
        if let Some(sub) = sub_box {
            out.descent = out.descent.max(down + sub.descent);
            out.ascent = out.ascent.max(sub.ascent - down);
            out.width = out.width.max(x + sub.width);
            out.place(sub, x, down);
        }
        out.width += 0.04 * size;
        out
    }

    /// ∑ and lim: scripts centred below and above.
    fn limits(
        &self,
        base: MathBox,
        sub: Option<MathBox>,
        sup: Option<MathBox>,
        size: f32,
    ) -> MathBox {
        let gap = 0.12 * size;
        let width = [
            Some(base.width),
            sub.as_ref().map(|b| b.width),
            sup.as_ref().map(|b| b.width),
        ]
        .into_iter()
        .flatten()
        .fold(0.0, f32::max);
        let mut out = MathBox {
            width,
            ascent: base.ascent,
            descent: base.descent,
            pieces: Vec::new(),
        };
        let base_width = base.width;
        out.place(base, (width - base_width) / 2.0, 0.0);
        if let Some(sup) = sup {
            let y = -(out.ascent + gap + sup.descent);
            out.ascent += gap + sup.height();
            let w = sup.width;
            out.place(sup, (width - w) / 2.0, y);
        }
        if let Some(sub) = sub {
            let y = out.descent + gap + sub.ascent;
            out.descent += gap + sub.height();
            let w = sub.width;
            out.place(sub, (width - w) / 2.0, y);
        }
        out
    }

    fn radical(&self, index: Option<&Node>, body: &Node, size: f32) -> MathBox {
        let body = self.node(body, size);
        let rule = (size * 0.06).max(1.0);
        let top = -(body.ascent + size * 0.12 + rule);
        let bottom = body.descent + size * 0.06;
        let tick = size * 0.5;
        let mid = top + (bottom - top) * 0.62;
        let index = index.map(|node| self.node(node, (size * 0.55).max(MIN_SIZE)));
        let x0 = index
            .as_ref()
            .map_or(0.0, |index| (index.width - tick * 0.55).max(0.0));
        let body_x = x0 + tick + size * 0.06;
        let width = body_x + body.width + size * 0.08;
        let mut out = MathBox {
            width,
            ascent: -top + rule / 2.0,
            descent: bottom,
            pieces: vec![Piece::Path {
                points: vec![
                    pos2(x0, mid),
                    pos2(x0 + tick * 0.28, mid - size * 0.05),
                    pos2(x0 + tick * 0.6, bottom),
                    pos2(x0 + tick, top),
                    pos2(width, top),
                ],
                width: rule,
            }],
        };
        if let Some(index) = index {
            // The index sits in the crook of the sign.
            let y = mid - size * 0.08 - index.descent;
            out.ascent = out.ascent.max(index.ascent - y);
            let w = index.width;
            out.place(index, (x0 + tick * 0.55 - w).max(0.0), y);
        }
        out.place(body, body_x, 0.0);
        out
    }

    fn fenced(&self, open: Option<char>, close: Option<char>, body: &Node, size: f32) -> MathBox {
        let body = self.node(body, size);
        self.fence_box(open, close, body, size)
    }

    /// `body`, already laid out, between delimiters that grow with it.
    fn fence_box(
        &self,
        open: Option<char>,
        close: Option<char>,
        body: MathBox,
        size: f32,
    ) -> MathBox {
        let plain = self.text("(", size, false);
        let tall = body.height() > plain.height() * 1.25;
        let top = -(body.ascent.max(plain.ascent) + size * 0.06);
        let bottom = body.descent.max(plain.descent) + size * 0.06;
        let mut out = MathBox {
            width: 0.0,
            ascent: body.ascent,
            descent: body.descent,
            pieces: Vec::new(),
        };
        let mut x = 0.0;
        if let Some(open) = open {
            let delimiter = self.delimiter(open, tall, top, bottom, size);
            out.ascent = out.ascent.max(delimiter.ascent);
            out.descent = out.descent.max(delimiter.descent);
            let w = delimiter.width;
            out.place(delimiter, x, 0.0);
            x += w + size * 0.04;
        }
        let body_width = body.width;
        out.place(body, x, 0.0);
        x += body_width;
        if let Some(close) = close {
            x += size * 0.04;
            let delimiter = self.delimiter(close, tall, top, bottom, size);
            out.ascent = out.ascent.max(delimiter.ascent);
            out.descent = out.descent.max(delimiter.descent);
            let w = delimiter.width;
            out.place(delimiter, x, 0.0);
            x += w;
        }
        out.width = x;
        out
    }

    /// A bracket as tall as `top..bottom`: the font's glyph when the contents are one
    /// line high, a drawn curve or line when they are taller.
    fn delimiter(&self, symbol: char, tall: bool, top: f32, bottom: f32, size: f32) -> MathBox {
        if !tall {
            return self.text(&symbol.to_string(), size, false);
        }
        let width = if symbol == '|' {
            size * 0.22
        } else {
            size * 0.36
        };
        let (l, r) = (width * 0.2, width * 0.8);
        let (c, mid) = (width * 0.5, (top + bottom) / 2.0);
        let points = match symbol {
            '(' => curve(pos2(r, top), pos2(2.0 * l - r, mid), pos2(r, bottom)),
            ')' => curve(pos2(l, top), pos2(2.0 * r - l, mid), pos2(l, bottom)),
            '[' => vec![pos2(r, top), pos2(l, top), pos2(l, bottom), pos2(r, bottom)],
            ']' => vec![pos2(l, top), pos2(r, top), pos2(r, bottom), pos2(l, bottom)],
            '{' => vec![
                pos2(r, top),
                pos2(c, top + size * 0.12),
                pos2(c, mid - size * 0.1),
                pos2(l, mid),
                pos2(c, mid + size * 0.1),
                pos2(c, bottom - size * 0.12),
                pos2(r, bottom),
            ],
            '}' => vec![
                pos2(l, top),
                pos2(c, top + size * 0.12),
                pos2(c, mid - size * 0.1),
                pos2(r, mid),
                pos2(c, mid + size * 0.1),
                pos2(c, bottom - size * 0.12),
                pos2(l, bottom),
            ],
            _ => vec![pos2(c, top), pos2(c, bottom)],
        };
        MathBox {
            width,
            ascent: -top,
            descent: bottom,
            pieces: vec![Piece::Path {
                points,
                width: (size * 0.06).max(1.0),
            }],
        }
    }
}

/// A quadratic curve from `a` to `b` bending towards `control`, as points.
fn curve(a: Pos2, control: Pos2, b: Pos2) -> Vec<Pos2> {
    (0..=16)
        .map(|step| {
            let t = step as f32 / 16.0;
            let u = 1.0 - t;
            pos2(
                u * u * a.x + 2.0 * u * t * control.x + t * t * b.x,
                u * u * a.y + 2.0 * u * t * control.y + t * t * b.y,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(text: &str) -> Node {
        Node::Ident {
            text: text.into(),
            italic: true,
        }
    }

    /// Runs `f` inside one egui pass with Folio's fonts installed (no GPU needed).
    fn with_ctx<R>(f: impl FnOnce(&egui::Context) -> R) -> R {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut f = Some(f);
        let mut out = None;
        ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(f) = f.take() {
                out = Some(f(ui.ctx()));
            }
        })
        .drop_without_applying_deltas();
        out.expect("ran one pass")
    }

    #[test]
    fn errors_read_in_french() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::Fr);
        assert_eq!(
            parse("x^").unwrap_err().message,
            "Il manque quelque chose à la fin"
        );
        // An open "(" is allowed while typing; an open "{" is not. The spaces inside « »
        // are no-break spaces.
        assert_eq!(
            parse("{x").unwrap_err().message,
            "Il manque «\u{a0}}\u{a0}»"
        );
        assert!(parse("\\frac")
            .unwrap_err()
            .message
            .starts_with("\\frac doit être suivi de"));
    }

    /// Previews show an equation typed in LaTeX as linear text, and leave prose, paths and
    /// linear equations as they are.
    #[test]
    fn previews_read_latex_as_linear_text() {
        let readable = |source: &str| readable_text(source).into_owned();
        assert_eq!(readable(r"A=\pi r^2"), "A = πr^2");
        assert_eq!(
            readable(r"x=\frac{-b\pm\sqrt{b^2-4ac}}{2a}"),
            "x = (−b±√(b^2−4ac))/(2a)"
        );
        assert_eq!(
            readable(r"\sin\alpha\pm\sin\beta=2\sin\frac{1}{2}(\alpha\pm\beta)"),
            "sin α±sin β = 2sin 1/2(α±β)"
        );
        assert_eq!(
            readable(r"(x+a)^n=\sum_{k=0}^{n}\binom{n}{k}x^k a^{n-k}"),
            "(x+a)^n = ∑_(k = 0)^n(n¦k)x^ka^(n−k)"
        );
        assert_eq!(readable(r"\sqrt[3]{x}+\text{area}"), "∛x+area");
        // Every ready-made equation reads without a backslash.
        for (_, source) in READY_EQUATIONS {
            let text = readable(source);
            assert!(!text.contains('\\'), "{source} reads {text}");
        }
        // The lines around an equation are kept, each as it was.
        assert_eq!(
            readable("Roots\nx=\\frac{1}{2}\nDone"),
            "Roots\nx = 1/2\nDone"
        );
        for kept in [
            "a^2 + b^2 = c^2",
            r"The angle \alpha is acute",
            r"C:\Users\me",
            r"\mathbb{R}",
            r"\frac{1}{",
        ] {
            assert_eq!(readable(kept), kept);
        }
        assert!(matches!(readable_text("no maths"), Cow::Borrowed(_)));
    }

    #[test]
    fn linear_format_and_latex_parse_alike() {
        let expected = Node::Row(vec![
            Node::Scripts {
                base: Box::new(ident("x")),
                sub: None,
                sup: Some(Box::new(Node::Number("2".into()))),
            },
            Node::Op('+'),
            Node::Frac(
                Box::new(Node::Number("1".into())),
                Box::new(Node::Number("2".into())),
            ),
        ]);
        assert_eq!(parse("x^2+1/2"), Ok(expected.clone()));
        assert_eq!(parse("x^{2} + \\frac{1}{2}"), Ok(expected));
        // Parentheses around a fraction's operands are dropped.
        assert_eq!(parse("(a+b)/c"), parse("\\frac{a+b}{c}"));
        assert_eq!(parse("sqrt(x+1)"), parse("\\sqrt{x+1}"));
        assert_eq!(parse("√(x+1)"), parse("\\sqrt{x+1}"));
        // Function names are upright; other letters are separate variables.
        assert_eq!(
            parse("sin x"),
            Ok(Node::Row(vec![
                Node::Ident {
                    text: "sin".into(),
                    italic: false
                },
                ident("x"),
            ]))
        );
        assert_eq!(parse("ab"), Ok(Node::Row(vec![ident("a"), ident("b")])));
        assert_eq!(parse("3,5"), Ok(Node::Number("3,5".into())));
        assert_eq!(
            parse("\\Delta"),
            Ok(Node::Ident {
                text: "Δ".into(),
                italic: false
            })
        );
        assert_eq!(parse("a <= b"), parse("a \\le b"));
    }

    #[test]
    fn malformed_input_is_an_error_not_a_panic() {
        for bad in [
            "\\frac{a}",
            "x^",
            "{x",
            "x}",
            "1/",
            "_2",
            "\\sqrt[3",
            "\\text{abc",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} should not parse");
        }
        for tolerated in ["(x+1", "x)", "|x", "a+", "\\unknown x", "[0,1)"] {
            assert!(parse(tolerated).is_ok(), "{tolerated:?} should parse");
        }
        let deep = format!("{}x", "(".repeat(500));
        assert!(
            parse(&deep).is_err(),
            "deep nesting is refused, not a stack overflow"
        );
        let braces = "{".repeat(10_000);
        assert!(parse(&braces).is_err());
        // Chained scripts and fractions nest too: refused, not a stack overflow.
        assert!(parse("x^1^1").is_ok());
        assert!(parse("1/2/3").is_ok());
        let scripts = format!("x{}", "^1".repeat(10_000));
        assert!(
            parse(&scripts).is_err(),
            "a long chain of scripts is refused"
        );
        let fractions = format!("1{}", "/1".repeat(10_000));
        assert!(
            parse(&fractions).is_err(),
            "a long chain of fractions is refused"
        );
        let mixed = format!("x{}", "_1^2/3".repeat(5_000));
        assert!(parse(&mixed).is_err());
        // Every short combination of awkward characters parses or fails cleanly.
        let parts = [
            "\\", "{", "}", "(", ")", "[", "]", "|", "^", "_", "/", "√", "∑", "x", "1", " ",
        ];
        for a in parts {
            for b in parts {
                for c in parts {
                    let _ = parse(&format!("{a}{b}{c}"));
                }
            }
        }
    }

    #[test]
    fn latex_round_trips() {
        for source in [
            "x^2+1/2",
            "\\frac{a+b}{c}",
            "(a+b)/c",
            "\\sqrt[3]{x^2}",
            "√(x+1)",
            "\\sum_{i=1}^{n} i^2",
            "\\int_0^1 x dx",
            "\\lim_{x\\to0} \\frac{\\sin x}{x}",
            "|x-1| \\le 3",
            "\\alpha + \\Delta \\cdot \\pi",
            "a_n^2",
            "e^{-x}",
            "f(x) = 3,5x",
            "\\text{if } x > 0",
            "\\left( \\frac{1}{2} \\right)^2",
            "x^-1",
        ] {
            let node = parse(source).unwrap();
            let latex = to_latex(&node);
            assert_eq!(parse(&latex), Ok(node), "{source:?} → {latex:?}");
        }
        assert_eq!(latex_of("x^2+1/2"), "x^{2}+\\frac{1}{2}");
        // Commands this parser doesn't know are written back as typed, with their argument.
        assert_eq!(latex_of("\\mathbb{R}"), "\\mathbb R");
        assert_eq!(latex_of("\\vec{AB}"), "\\vec {AB}");
        assert_eq!(latex_of("\\hat{x}+1"), "\\hat x+1");
        // An open fence still gets its (invisible) \\right.
        assert_eq!(latex_of("(x+1"), "\\left(x+1\\right.");
        assert_eq!(latex_of("|x|"), "\\left|x\\right|");
        for source in ["\\mathbb{R}", "\\vec{AB} + 1", "(x+1", "[0,1)", "|x"] {
            let node = parse(source).unwrap();
            let latex = to_latex(&node);
            assert_eq!(parse(&latex), Ok(node), "{source:?} → {latex:?}");
        }
        assert_eq!(
            latex_of("\\frac{a}"),
            "\\frac{a}",
            "unreadable source is kept"
        );
    }

    #[test]
    fn ready_equations_parse_without_unknown_commands() {
        fn unknown(node: &Node) -> Option<String> {
            match node {
                Node::Command(name) => Some(name.clone()),
                Node::Row(items) => items.iter().find_map(unknown),
                Node::Frac(a, b) | Node::Binom(a, b) => unknown(a).or_else(|| unknown(b)),
                Node::Scripts { base, sub, sup } => unknown(base)
                    .or_else(|| sub.as_deref().and_then(unknown))
                    .or_else(|| sup.as_deref().and_then(unknown)),
                Node::Sqrt { index, body } => {
                    index.as_deref().and_then(unknown).or_else(|| unknown(body))
                }
                Node::Fenced { body, .. } => unknown(body),
                Node::Number(_) | Node::Ident { .. } | Node::Text(_) | Node::Op(_) => None,
                Node::BigOp(_) => None,
            }
        }
        for (name, source) in READY_EQUATIONS {
            let node = parse(source).unwrap_or_else(|error| panic!("{name}: {error:?}"));
            assert_eq!(unknown(&node), None, "{name}: {source}");
            for lang in crate::i18n::Lang::ALL {
                assert_ne!(crate::i18n::tr_in(lang, name), name, "{name} is named");
            }
        }
        let binomial = parse(READY_EQUATIONS[1].1).unwrap();
        assert!(format!("{binomial:?}").contains("Binom"), "\\binom is read");
    }

    #[test]
    fn binom_parses_renders_and_round_trips() {
        let node = parse("\\binom{n}{k}").unwrap();
        assert_eq!(
            node,
            Node::Binom(Box::new(ident("n")), Box::new(ident("k")))
        );
        assert_eq!(to_latex(&node), "\\binom{n}{k}");
        assert_eq!(parse("\\dbinom{n}{k}"), Ok(node.clone()));
        let busy = "\\sum_{k=0}^{n}\\binom{n}{k}x^k a^{n-k}";
        let latex = latex_of(busy);
        assert_eq!(parse(&latex), parse(busy), "{latex:?}");
        let mathml = mathml_of("\\binom{n}{k}");
        assert!(mathml.contains("<mfrac linethickness=\"0\">"), "{mathml}");
        assert!(mathml.contains("<mo>(</mo>") && mathml.contains("<mo>)</mo>"));
        with_ctx(|ctx| {
            let line = layout_source(ctx, "n", 22.0);
            let binom = layout_source(ctx, "\\binom{n}{k}", 22.0);
            assert!(binom.size.y > line.size.y * 1.8, "n over k");
            assert!(
                !binom
                    .pieces
                    .iter()
                    .any(|piece| matches!(piece, Piece::Rule(_))),
                "no fraction bar"
            );
            let paths = binom
                .pieces
                .iter()
                .filter(|piece| matches!(piece, Piece::Path { .. }))
                .count();
            assert_eq!(paths, 2, "both parentheses stretch around it");
        });
    }

    #[test]
    fn text_arguments_are_kept_as_typed() {
        // Apostrophes, hyphens, arrows and runs of spaces are text here, not maths.
        let source = "\\text{l'eau -> a  b, x<=y} + 1";
        let Ok(Node::Row(items)) = parse(source) else {
            panic!("{source:?} should parse to a row");
        };
        assert_eq!(items[0], Node::Text("l'eau -> a  b, x<=y".into()));
        assert_eq!(
            parse("\\operatorname{arg-max}"),
            Ok(Node::Ident {
                text: "arg-max".into(),
                italic: false
            })
        );
        assert_eq!(parse("\\text{a {b} c}"), Ok(Node::Text("a {b} c".into())));
        assert_eq!(latex_of("\\text{l'été}"), "\\text{l'été}");
        assert!(mathml_of("\\text{l'eau}").contains("<mtext>l'eau</mtext>"));
    }

    #[test]
    fn mathml_export() {
        assert_eq!(
            mathml_of("x^2+1/2"),
            "<math display=\"block\"><mrow><msup><mi>x</mi><mn>2</mn></msup><mo>+</mo>\
             <mfrac><mn>1</mn><mn>2</mn></mfrac></mrow></math>"
        );
        assert!(mathml_of("\\sum_{i=1}^{n} i").contains("<munderover><mo largeop=\"true\">∑</mo>"));
        assert!(mathml_of("\\sqrt[3]{x}").contains("<mroot><mi>x</mi><mn>3</mn></mroot>"));
        assert!(mathml_of("a<b").contains("<mo>&lt;</mo>"));
        assert_eq!(
            mathml_of("\\frac{a}"),
            "<math display=\"block\"><mtext>\\frac{a}</mtext></math>"
        );
    }

    #[test]
    fn layout_stacks_scripts_fractions_and_roots() {
        with_ctx(|ctx| {
            let x = layout_source(ctx, "x", 22.0);
            let square = layout_source(ctx, "x^2", 22.0);
            assert!(square.size.x > x.size.x && square.size.y > x.size.y);

            let half = layout_source(ctx, "1/2", 22.0);
            assert!(half.size.y > layout_source(ctx, "1", 22.0).size.y * 1.8);
            assert!(half
                .pieces
                .iter()
                .any(|piece| matches!(piece, Piece::Rule(_))));
            let ys: Vec<f32> = half
                .pieces
                .iter()
                .filter_map(|piece| match piece {
                    Piece::Text { at, .. } => Some(at.y),
                    _ => None,
                })
                .collect();
            assert!(ys[0] < ys[1], "numerator above denominator");

            let root = layout_source(ctx, "\\sqrt{x}", 22.0);
            assert!(root
                .pieces
                .iter()
                .any(|piece| matches!(piece, Piece::Path { .. })));
            let tall = layout_source(ctx, "(\\frac{1}{2})", 22.0);
            let paths = tall
                .pieces
                .iter()
                .filter(|piece| matches!(piece, Piece::Path { .. }))
                .count();
            assert_eq!(paths, 2, "both parentheses stretch around a fraction");

            let double = layout_source(ctx, "x^2", 44.0);
            assert!((double.size.x / square.size.x - 2.0).abs() < 0.3);

            let broken = layout_source(ctx, "\\frac{a}", 22.0);
            assert!(broken.error.is_some() && broken.size.x > 0.0);
            assert!(layout_source(ctx, "   ", 22.0).is_empty());
            // No prefix of a busy equation panics while it is being typed.
            let busy = "\\sum_{i=1}^{n} \\frac{\\sqrt[3]{x^2}}{|y|} + (a/b)^2 - \\int_0^1 f(x)dx";
            for (end, _) in busy.char_indices() {
                let _ = layout_source(ctx, &busy[..end], 22.0);
            }
        });
    }
}
