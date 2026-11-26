//! the desktop notebook's inline maths (reference §3.3): an expression typed before "=", followed by
//! Space or Enter, gets its answer written after the "=", as "95+83+416=" becomes
//! "95+83+416=594". Numbers, + − × ÷ / * ^, % (per cent) and ! (factorial), brackets, pi,
//! and the desktop notebook's functions under their English or French names, in the interface
//! language's decimal style. Anything else gives no answer: it never guesses.

/// Deeper nesting than this gives no answer, so pasted junk can't overflow the stack.
const MAX_DEPTH: usize = 32;
/// At most this many chars before the "=" are read, so a long paragraph stays cheap.
const MAX_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Name(String),
    Op(char),
    Open,
    Close,
    Separator,
}

/// `text` as tokens, or `None` at a character no expression holds. With a decimal comma
/// ("2,5", French) arguments are separated by ";", else by "," or ";".
fn tokens(text: &str, decimal_comma: bool) -> Option<Vec<Token>> {
    let chars: Vec<char> = text.chars().collect();
    let point = |c: char| c == '.' || (decimal_comma && c == ',');
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit()
            || (point(c) && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || point(chars[i])) {
                i += 1;
            }
            let number: String = chars[start..i]
                .iter()
                .map(|&c| if point(c) { '.' } else { c })
                .collect();
            out.push(Token::Number(number.parse().ok()?));
        } else if c.is_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_alphanumeric() {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            out.push(Token::Name(name.to_lowercase()));
        } else {
            out.push(match c {
                '+' => Token::Op('+'),
                '-' | '−' => Token::Op('-'),
                '*' | '×' | '·' => Token::Op('*'),
                '/' | '÷' => Token::Op('/'),
                '^' | '%' | '!' => Token::Op(c),
                '(' => Token::Open,
                ')' => Token::Close,
                ';' => Token::Separator,
                ',' if !decimal_comma => Token::Separator,
                _ => return None,
            });
            i += 1;
        }
    }
    Some(out)
}

/// A recursive-descent reading of the tokens, worked out as it goes.
struct Parser {
    tokens: Vec<Token>,
    at: usize,
    depth: usize,
}

impl Parser {
    fn eat(&mut self, token: &Token) -> bool {
        let found = self.tokens.get(self.at) == Some(token);
        if found {
            self.at += 1;
        }
        found
    }

    /// `f` one level deeper: `None` past `MAX_DEPTH`.
    fn deeper(&mut self, f: impl FnOnce(&mut Parser) -> Option<f64>) -> Option<f64> {
        if self.depth >= MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let value = f(self);
        self.depth -= 1;
        value
    }

    /// sum := product (("+" | "-") product)*
    fn sum(&mut self) -> Option<f64> {
        let mut value = self.product()?;
        loop {
            if self.eat(&Token::Op('+')) {
                value += self.product()?;
            } else if self.eat(&Token::Op('-')) {
                value -= self.product()?;
            } else {
                return Some(value);
            }
        }
    }

    /// product := signed (("*" | "/") signed)*. Dividing by zero gives no answer.
    fn product(&mut self) -> Option<f64> {
        let mut value = self.signed()?;
        loop {
            if self.eat(&Token::Op('*')) {
                value *= self.signed()?;
            } else if self.eat(&Token::Op('/')) {
                let divisor = self.signed()?;
                if divisor == 0.0 {
                    return None;
                }
                value /= divisor;
            } else {
                return Some(value);
            }
        }
    }

    /// signed := ("-" | "+") signed | power. A sign binds looser than "^": -2^2 is -4.
    fn signed(&mut self) -> Option<f64> {
        self.deeper(|parser| {
            if parser.eat(&Token::Op('-')) {
                return Some(-parser.signed()?);
            }
            if parser.eat(&Token::Op('+')) {
                return parser.signed();
            }
            parser.power()
        })
    }

    /// power := postfix ("^" signed)?, so 2^3^2 is 2^9 and 2^-1 is 0.5.
    fn power(&mut self) -> Option<f64> {
        let base = self.postfix()?;
        if self.eat(&Token::Op('^')) {
            return Some(base.powf(self.signed()?));
        }
        Some(base)
    }

    /// postfix := atom ("%" | "!")*
    fn postfix(&mut self) -> Option<f64> {
        let mut value = self.atom()?;
        loop {
            if self.eat(&Token::Op('%')) {
                value /= 100.0;
            } else if self.eat(&Token::Op('!')) {
                value = factorial(value)?;
            } else {
                return Some(value);
            }
        }
    }

    /// atom := number | "(" sum ")" | name | name "(" (sum (separator sum)*)? ")"
    fn atom(&mut self) -> Option<f64> {
        let token = self.tokens.get(self.at)?.clone();
        self.at += 1;
        match token {
            Token::Number(value) => Some(value),
            Token::Open => {
                let value = self.deeper(Parser::sum)?;
                self.eat(&Token::Close).then_some(value)
            }
            Token::Name(name) => {
                let mut args = Vec::new();
                if self.eat(&Token::Open) && !self.eat(&Token::Close) {
                    loop {
                        args.push(self.deeper(Parser::sum)?);
                        if self.eat(&Token::Close) {
                            break;
                        }
                        if !self.eat(&Token::Separator) {
                            return None;
                        }
                    }
                }
                function(&name, &args)
            }
            _ => None,
        }
    }
}

/// the desktop notebook's functions, by their English or French name. Angles are in radians, as in
/// the desktop notebook (DEG and RAD convert). Out of their domain they give no answer.
fn function(name: &str, args: &[f64]) -> Option<f64> {
    let value = match (name, args) {
        ("pi", []) => std::f64::consts::PI,
        ("abs", [x]) => x.abs(),
        ("sqrt" | "racine", [x]) if *x >= 0.0 => x.sqrt(),
        ("sin", [x]) => x.sin(),
        ("cos", [x]) => x.cos(),
        ("tan", [x]) => x.tan(),
        ("asin", [x]) if x.abs() <= 1.0 => x.asin(),
        ("acos", [x]) if x.abs() <= 1.0 => x.acos(),
        ("atan", [x]) => x.atan(),
        ("deg" | "degres" | "degrés", [x]) => x.to_degrees(),
        ("rad" | "radians", [x]) => x.to_radians(),
        ("ln", [x]) if *x > 0.0 => x.ln(),
        ("log" | "log10", [x]) if *x > 0.0 => x.log10(),
        ("log2", [x]) if *x > 0.0 => x.log2(),
        ("mod", [x, y]) if *y != 0.0 => x.rem_euclid(*y),
        ("sign" | "signe", [x]) => {
            if *x == 0.0 {
                0.0
            } else {
                x.signum()
            }
        }
        // The payment per period of a loan, negative as Excel's and the desktop notebook's PMT.
        ("pmt" | "vpm", [rate, periods, present]) => {
            if *rate == 0.0 {
                -present / periods
            } else {
                -present * rate / (1.0 - (1.0 + rate).powf(-periods))
            }
        }
        _ => return None,
    };
    value.is_finite().then_some(value)
}

/// n! for a whole n from 0 to 170 (171! is past f64).
fn factorial(value: f64) -> Option<f64> {
    if value < 0.0 || value.fract() != 0.0 || value > 170.0 {
        return None;
    }
    Some((1..=value as u32).map(f64::from).product())
}

/// The value of `text`, or `None` when it is not one whole expression, does no
/// calculation (a lone number or name), or has no finite value.
pub fn evaluate(text: &str, decimal_comma: bool) -> Option<f64> {
    let tokens = tokens(text, decimal_comma)?;
    if !tokens
        .iter()
        .any(|token| matches!(token, Token::Op(_) | Token::Open))
    {
        return None;
    }
    let mut parser = Parser {
        tokens,
        at: 0,
        depth: 0,
    };
    let value = parser.sum()?;
    (parser.at == parser.tokens.len() && value.is_finite()).then_some(value)
}

/// The answer to write after the "=" that ends `before` (a paragraph's text up to the
/// caret), or `None`. The expression is the longest stretch before the "=" that starts a
/// word (after a space, "=" or ":") and works out, so "Total: 95+83+416=" gives "594" and
/// "x = 3*4 =" gives "12". A stretch the text before runs on into is none ("1 500 + 300",
/// see [`word_start`]).
pub fn answer(before: &str, decimal_comma: bool) -> Option<String> {
    let body = before.strip_suffix('=')?;
    if body.ends_with('=') || body.trim().is_empty() {
        return None;
    }
    // Only the last MAX_CHARS chars are read: where they begin mid-word, nothing starts
    // there.
    let cut = body
        .char_indices()
        .rev()
        .nth(MAX_CHARS)
        .map_or(0, |(at, c)| at + c.len_utf8());
    let boundary = |c: char| c.is_whitespace() || c == '=' || c == ':';
    std::iter::once(cut)
        .filter(|&cut| cut == 0 || body[..cut].ends_with(boundary))
        .chain(
            body[cut..]
                .char_indices()
                .filter(|&(_, c)| boundary(c))
                .map(|(at, c)| cut + at + c.len_utf8()),
        )
        .filter(|&start| start < body.len() && word_start(body, start))
        .find_map(|start| evaluate(&body[start..], decimal_comma))
        .map(|value| format_number(value, decimal_comma))
}

/// Whether an expression may start at byte `start` of `body`, right after a space, "="
/// or ":". Not where the text before runs on into it: after a sign or "(", before a sign
/// that follows a number or ")", or before three digits that follow one (a space between
/// thousands). So "1 500 + 300" is no sum of "500 + 300" nor of "+ 300": it gives no
/// answer rather than a wrong one.
fn word_start(body: &str, start: usize) -> bool {
    let Some(before) = body[..start].trim_end().chars().next_back() else {
        return true;
    };
    let sign = |c: char| matches!(c, '+' | '-' | '−' | '*' | '×' | '·' | '/' | '÷' | '^');
    if sign(before) || before == '(' {
        return false;
    }
    let rest = body[start..].trim_start();
    if (before.is_ascii_digit() || before == ')') && rest.starts_with(sign) {
        return false;
    }
    !(before.is_ascii_digit() && rest.chars().take_while(char::is_ascii_digit).count() == 3)
}

/// `value` as an answer: at most ten decimals, no trailing zeros, no "-0", in the
/// interface's decimal style ("2.5", "2,5"); from 10^15 up, in scientific notation.
pub fn format_number(value: f64, decimal_comma: bool) -> String {
    let text = if value.abs() >= 1e15 {
        format!("{value:e}")
    } else {
        let fixed = format!("{value:.10}");
        match fixed.trim_end_matches('0').trim_end_matches('.') {
            "-0" | "" => "0".to_string(),
            trimmed => trimmed.to_string(),
        }
    };
    if decimal_comma {
        text.replace('.', ",")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn en(text: &str) -> Option<String> {
        answer(text, false)
    }

    fn fr(text: &str) -> Option<String> {
        answer(text, true)
    }

    #[test]
    fn sums_and_operators_work_out_as_notebook_does() {
        assert_eq!(en("95+83+416=").as_deref(), Some("594"));
        assert_eq!(en("2*(3+4)=").as_deref(), Some("14"));
        assert_eq!(en("2^10=").as_deref(), Some("1024"));
        assert_eq!(
            en("-2^2=").as_deref(),
            Some("-4"),
            "a sign binds looser than ^"
        );
        assert_eq!(en("2^-1=").as_deref(), Some("0.5"));
        assert_eq!(en("2^3^2=").as_deref(), Some("512"), "^ goes right to left");
        assert_eq!(en("10/4=").as_deref(), Some("2.5"));
        assert_eq!(
            en("7−2×3÷2=").as_deref(),
            Some("4"),
            "the typographic signs"
        );
        assert_eq!(en("50%*200=").as_deref(), Some("100"));
        assert_eq!(en("5!=").as_deref(), Some("120"));
        assert_eq!(en("((((((((((1+1))))))))))=").as_deref(), Some("2"));
        assert_eq!(en("0.1+0.2=").as_deref(), Some("0.3"));
        assert_eq!(en("1/3=").as_deref(), Some("0.3333333333"));
        assert_eq!(en("2/3=").as_deref(), Some("0.6666666667"));
    }

    #[test]
    fn functions_take_english_and_french_names() {
        assert_eq!(en("sqrt(16)=").as_deref(), Some("4"));
        assert_eq!(fr("RACINE(16)=").as_deref(), Some("4"));
        assert_eq!(en("mod(17,5)=").as_deref(), Some("2"));
        assert_eq!(fr("MOD(17;5)=").as_deref(), Some("2"));
        assert_eq!(en("sin(0)=").as_deref(), Some("0"));
        assert_eq!(en("cos(pi)=").as_deref(), Some("-1"));
        assert_eq!(en("sin(pi)=").as_deref(), Some("0"), "no 1.2e-16");
        assert_eq!(en("deg(pi)=").as_deref(), Some("180"));
        assert_eq!(en("rad(180)=").as_deref(), Some("3.1415926536"));
        assert_eq!(en("log(1000)=").as_deref(), Some("3"));
        assert_eq!(en("ln(1)=").as_deref(), Some("0"));
        assert_eq!(en("log2(8)=").as_deref(), Some("3"));
        assert_eq!(en("abs(-3)+sign(-2)=").as_deref(), Some("2"));
        assert_eq!(en("pmt(0,10,1000)=").as_deref(), Some("-100"));
        assert_eq!(fr("VPM(0;10;1000)=").as_deref(), Some("-100"));
        assert_eq!(en("pi()*2=").as_deref(), Some("6.2831853072"));
    }

    #[test]
    fn the_decimal_comma_follows_the_language() {
        assert_eq!(fr("2,5+1=").as_deref(), Some("3,5"));
        assert_eq!(fr("10/4=").as_deref(), Some("2,5"));
        assert_eq!(en("1,5+1="), None, "in English a comma separates arguments");
        assert_eq!(format_number(1234.5, true), "1234,5");
    }

    #[test]
    fn the_expression_is_the_end_of_the_line() {
        assert_eq!(en("Total: 95+83+416=").as_deref(), Some("594"));
        assert_eq!(en("x=3*4=").as_deref(), Some("12"));
        assert_eq!(en("x = 3 * 4 =").as_deref(), Some("12"));
        assert_eq!(en("3 + 4 = 7 and 2+2=").as_deref(), Some("4"));
        let long = format!("{} 2+2=", "word ".repeat(400));
        assert_eq!(en(&long).as_deref(), Some("4"), "a long line stays cheap");
        // A number before the sum that is no part of it.
        assert_eq!(en("Exercise 4 2+2=").as_deref(), Some("4"));
        assert_eq!(en("2) 3+4=").as_deref(), Some("7"));
        assert_eq!(fr("Prix : 2,5 + 1 =").as_deref(), Some("3,5"));
    }

    /// The text before a start running on into it gives no answer, never part of one:
    /// "1 500 + 300 =" is not "800".
    #[test]
    fn no_answer_for_part_of_an_expression() {
        assert_eq!(
            fr("Prix : 1 500 + 300 ="),
            None,
            "a space between thousands"
        );
        assert_eq!(en("1 500+300="), None);
        assert_eq!(fr("1 000,5 + 1 ="), None);
        assert_eq!(en("f(1 + 2) * 3="), None);
        assert_eq!(en("( 2+2="), None);
        // Past the last MAX_CHARS chars read: what was cut off may be part of the sum.
        let long = format!("{}+2345=", "1".repeat(MAX_CHARS + 10));
        assert_eq!(en(&long), None);
        let long = format!("{}1=", "1 + ".repeat(100));
        assert_eq!(en(&long), None);
    }

    #[test]
    fn nothing_is_written_without_a_sure_answer() {
        for text in [
            "1/0=",
            "sqrt(-1)=",
            "log(0)=",
            "171!=",
            "2.5!=",
            "2+=",
            "abc=",
            "5=",
            "pi=",
            "=",
            "2+2",
            "2==",
            "∑=",
            "1e5=",
            "2 3=",
            "sqrt 4=",
            "mod(1)=",
            "(1+2=",
        ] {
            assert_eq!(en(text), None, "{text}");
        }
        let deep = format!("{}1{}=", "(".repeat(40), ")".repeat(40));
        assert_eq!(en(&deep), None, "too deep");
        assert_eq!(evaluate("", false), None);
    }

    #[test]
    fn answers_are_written_plainly() {
        assert_eq!(format_number(-0.0, false), "0");
        assert_eq!(format_number(1e20, false), "1e20");
        assert_eq!(format_number(2.0, false), "2");
        assert_eq!(format_number(-1.25, false), "-1.25");
    }
}
