use anyhow::{Result, bail, ensure};

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    Name(String),
    Left,
    Right,
    Comma,
    Op(&'static str),
    End,
}

#[derive(Clone, Debug)]
enum Instruction {
    Number(f64),
    X,
    Unary(&'static str),
    Binary(&'static str),
    Function(String, usize),
}

#[derive(Clone, Debug)]
pub struct Expression {
    code: Vec<Instruction>,
}

const FUNCTIONS: &[(&str, usize, usize)] = &[
    ("sin", 1, 1),
    ("cos", 1, 1),
    ("tan", 1, 1),
    ("asin", 1, 1),
    ("acos", 1, 1),
    ("atan", 1, 1),
    ("arcsin", 1, 1),
    ("arccos", 1, 1),
    ("arctan", 1, 1),
    ("atan2", 2, 2),
    ("arctan2", 2, 2),
    ("sinh", 1, 1),
    ("cosh", 1, 1),
    ("tanh", 1, 1),
    ("asinh", 1, 1),
    ("acosh", 1, 1),
    ("atanh", 1, 1),
    ("arcsinh", 1, 1),
    ("arccosh", 1, 1),
    ("arctanh", 1, 1),
    ("exp", 1, 1),
    ("exp2", 1, 1),
    ("expm1", 1, 1),
    ("log", 1, 1),
    ("ln", 1, 1),
    ("log2", 1, 1),
    ("log10", 1, 1),
    ("log1p", 1, 1),
    ("sqrt", 1, 1),
    ("cbrt", 1, 1),
    ("square", 1, 1),
    ("abs", 1, 1),
    ("absolute", 1, 1),
    ("fabs", 1, 1),
    ("sign", 1, 1),
    ("floor", 1, 1),
    ("ceil", 1, 1),
    ("trunc", 1, 1),
    ("rint", 1, 1),
    ("round", 1, 2),
    ("around", 1, 2),
    ("minimum", 2, 2),
    ("maximum", 2, 2),
    ("min", 2, 32),
    ("max", 2, 32),
    ("clip", 3, 3),
    ("where", 3, 3),
    ("power", 2, 2),
    ("pow", 2, 2),
    ("hypot", 2, 2),
    ("mod", 2, 2),
    ("remainder", 2, 2),
    ("fmod", 2, 2),
    ("degrees", 1, 1),
    ("rad2deg", 1, 1),
    ("radians", 1, 1),
    ("deg2rad", 1, 1),
    ("sinc", 1, 1),
    ("heaviside", 2, 2),
    ("isnan", 1, 1),
    ("isinf", 1, 1),
    ("isfinite", 1, 1),
    ("logical_not", 1, 1),
    ("logical_and", 2, 2),
    ("logical_or", 2, 2),
    ("logical_xor", 2, 2),
];

fn lex(text: &str) -> Result<Vec<Token>> {
    ensure!(
        text.len() <= 16_384,
        "Formula is too long (maximum 16384 bytes)"
    );
    let bytes = text.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        if bytes[i].is_ascii_digit() || bytes[i] == b'.' {
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            // A bare e is the constant; only absorb it as an exponent when a
            // signed/unsigned digit follows. This also permits 2e and 2x.
            if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
                let mut end = i + 1;
                if end < bytes.len() && matches!(bytes[end], b'+' | b'-') {
                    end += 1;
                }
                if end < bytes.len() && bytes[end].is_ascii_digit() {
                    i = end + 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let value = text[start..i]
                .parse::<f64>()
                .map_err(|_| anyhow::anyhow!("Invalid number at column {}", start + 1))?;
            result.push(Token::Number(value));
        } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
            {
                i += 1;
            }
            let name = &text[start..i];
            result.push(match name {
                "and" => Token::Op("and"),
                "or" => Token::Op("or"),
                "not" => Token::Op("not"),
                _ => Token::Name(name.to_owned()),
            });
        } else {
            let pair = bytes.get(i..i + 2);
            let two = match pair {
                Some(b"**") => Some("^"),
                Some(b"//") => Some("//"),
                Some(b"<=") => Some("<="),
                Some(b">=") => Some(">="),
                Some(b"==") => Some("=="),
                Some(b"!=") => Some("!="),
                _ => None,
            };
            if let Some(op) = two {
                result.push(Token::Op(op));
                i += 2;
            } else {
                result.push(match bytes[i] {
                    b'(' => Token::Left,
                    b')' => Token::Right,
                    b',' => Token::Comma,
                    b'+' => Token::Op("+"),
                    b'-' => Token::Op("-"),
                    b'*' => Token::Op("*"),
                    b'/' => Token::Op("/"),
                    b'%' => Token::Op("%"),
                    b'^' => Token::Op("^"),
                    b'<' => Token::Op("<"),
                    b'>' => Token::Op(">"),
                    b'&' => Token::Op("and"),
                    b'|' => Token::Op("or"),
                    _ => bail!("Unexpected character at column {}", i + 1),
                });
                i += 1;
            }
        }
        ensure!(result.len() <= 4096, "Formula has too many terms");
    }
    result.push(Token::End);
    Ok(result)
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
    code: Vec<Instruction>,
}

impl Parser {
    fn take(&mut self) -> Token {
        let token = self.tokens[self.position].clone();
        self.position += 1;
        token
    }
    fn current(&self) -> &Token {
        &self.tokens[self.position]
    }
    fn expression(&mut self, minimum: u8, depth: usize) -> Result<()> {
        ensure!(depth < 128, "Formula nesting is too deep");
        match self.take() {
            Token::Number(value) => self.code.push(Instruction::Number(value)),
            Token::Name(name) => {
                let name = name
                    .strip_prefix("np.")
                    .or_else(|| name.strip_prefix("numpy."))
                    .unwrap_or(&name);
                match name {
                    "x" => self.code.push(Instruction::X),
                    "pi" => self.code.push(Instruction::Number(std::f64::consts::PI)),
                    "e" => self.code.push(Instruction::Number(std::f64::consts::E)),
                    "tau" => self.code.push(Instruction::Number(std::f64::consts::TAU)),
                    "inf" | "Infinity" => self.code.push(Instruction::Number(f64::INFINITY)),
                    "nan" | "NaN" => self.code.push(Instruction::Number(f64::NAN)),
                    "True" => self.code.push(Instruction::Number(1.0)),
                    "False" => self.code.push(Instruction::Number(0.0)),
                    _ => {
                        let function = FUNCTIONS
                            .iter()
                            .find(|(function, _, _)| *function == name)
                            .ok_or_else(|| {
                                anyhow::anyhow!("Unknown function or variable: {name}")
                            })?;
                        ensure!(self.take() == Token::Left, "Use {name}(...) for a function");
                        let mut count = 0;
                        if *self.current() != Token::Right {
                            loop {
                                self.expression(0, depth + 1)?;
                                count += 1;
                                if *self.current() != Token::Comma {
                                    break;
                                }
                                self.take();
                            }
                        }
                        ensure!(
                            self.take() == Token::Right,
                            "Missing closing parenthesis in {name}(...)"
                        );
                        ensure!(
                            (function.1..=function.2).contains(&count),
                            "{name} expects {}{} arguments",
                            function.1,
                            if function.1 == function.2 {
                                String::new()
                            } else {
                                format!("–{}", function.2)
                            }
                        );
                        self.code
                            .push(Instruction::Function(name.to_owned(), count));
                    }
                }
            }
            Token::Left => {
                self.expression(0, depth + 1)?;
                ensure!(self.take() == Token::Right, "Missing closing parenthesis");
            }
            Token::Op(op @ ("+" | "-" | "not")) => {
                self.expression(if op == "not" { 3 } else { 9 }, depth + 1)?;
                self.code.push(Instruction::Unary(op));
            }
            _ => bail!("Expected a number, x, a function, or an opening parenthesis"),
        }
        loop {
            let implicit = matches!(
                self.current(),
                Token::Number(_) | Token::Name(_) | Token::Left
            );
            let op = if implicit {
                "*"
            } else if let Token::Op(op) = self.current() {
                *op
            } else {
                break;
            };
            let (left, right) = match op {
                "or" => (1, 2),
                "and" => (2, 3),
                "<" | ">" | "<=" | ">=" | "==" | "!=" => (3, 4),
                "+" | "-" => (5, 6),
                "*" | "/" | "//" | "%" => (7, 8),
                "^" => (11, 10),
                _ => break,
            };
            if left < minimum {
                break;
            }
            if !implicit {
                self.take();
            }
            self.expression(right, depth + 1)?;
            self.code.push(Instruction::Binary(op));
        }
        Ok(())
    }
}

impl Expression {
    pub fn parse(text: &str) -> Result<Self> {
        let text = text.trim();
        let text = text
            .strip_prefix('y')
            .and_then(|tail| tail.trim_start().strip_prefix('='))
            .unwrap_or(text);
        let mut parser = Parser {
            tokens: lex(text)?,
            position: 0,
            code: Vec::new(),
        };
        parser.expression(0, 0)?;
        ensure!(
            *parser.current() == Token::End,
            "Unexpected term or closing parenthesis"
        );
        Ok(Self { code: parser.code })
    }

    pub fn evaluate(&self, x: f64) -> f64 {
        self.evaluate_with_stack(x, &mut Vec::new())
    }

    pub fn evaluate_with_stack(&self, x: f64, stack: &mut Vec<f64>) -> f64 {
        stack.clear();
        for instruction in &self.code {
            match instruction {
                Instruction::Number(value) => stack.push(*value),
                Instruction::X => stack.push(x),
                Instruction::Unary(op) => {
                    let a = stack.last_mut().expect("compiled unary operand");
                    *a = match *op {
                        "-" => -*a,
                        "not" => boolean(*a == 0.0),
                        _ => *a,
                    };
                }
                Instruction::Binary(op) => {
                    let b = stack.pop().expect("compiled right operand");
                    let a = stack.last_mut().expect("compiled left operand");
                    *a = match *op {
                        "+" => *a + b,
                        "-" => *a - b,
                        "*" => *a * b,
                        "/" => *a / b,
                        "//" => (*a / b).floor(),
                        "%" => remainder(*a, b),
                        "^" => a.powf(b),
                        "<" => boolean(*a < b),
                        ">" => boolean(*a > b),
                        "<=" => boolean(*a <= b),
                        ">=" => boolean(*a >= b),
                        "==" => boolean(*a == b),
                        "!=" => boolean(*a != b),
                        "and" => boolean(*a != 0.0 && b != 0.0),
                        "or" => boolean(*a != 0.0 || b != 0.0),
                        _ => unreachable!(),
                    };
                }
                Instruction::Function(name, count) => {
                    let start = stack.len() - count;
                    let value = function(name, &stack[start..]);
                    stack.truncate(start);
                    stack.push(value);
                }
            }
        }
        stack[0]
    }
}

fn boolean(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}
fn remainder(a: f64, b: f64) -> f64 {
    let r = a % b;
    if r != 0.0 && r.is_sign_negative() != b.is_sign_negative() {
        r + b
    } else {
        r.copysign(b)
    }
}
fn function(name: &str, values: &[f64]) -> f64 {
    let a = values[0];
    let b = *values.get(1).unwrap_or(&0.0);
    match name {
        "sin" => a.sin(),
        "cos" => a.cos(),
        "tan" => a.tan(),
        "asin" | "arcsin" => a.asin(),
        "acos" | "arccos" => a.acos(),
        "atan" | "arctan" => a.atan(),
        "atan2" | "arctan2" => a.atan2(b),
        "sinh" => a.sinh(),
        "cosh" => a.cosh(),
        "tanh" => a.tanh(),
        "asinh" | "arcsinh" => a.asinh(),
        "acosh" | "arccosh" => a.acosh(),
        "atanh" | "arctanh" => a.atanh(),
        "exp" => a.exp(),
        "exp2" => a.exp2(),
        "expm1" => a.exp_m1(),
        "log" | "ln" => a.ln(),
        "log2" => a.log2(),
        "log10" => a.log10(),
        "log1p" => a.ln_1p(),
        "sqrt" => a.sqrt(),
        "cbrt" => a.cbrt(),
        "square" => a * a,
        "abs" | "absolute" | "fabs" => a.abs(),
        "sign" => {
            if a.is_nan() {
                a
            } else if a == 0.0 {
                0.0
            } else {
                a.signum()
            }
        }
        "floor" => a.floor(),
        "ceil" => a.ceil(),
        "trunc" => a.trunc(),
        "rint" => a.round_ties_even(),
        "round" | "around" => {
            let factor = 10_f64.powf(b);
            (a * factor).round_ties_even() / factor
        }
        "minimum" | "min" => {
            if values.iter().any(|v| v.is_nan()) {
                f64::NAN
            } else {
                values.iter().copied().fold(f64::INFINITY, f64::min)
            }
        }
        "maximum" | "max" => {
            if values.iter().any(|v| v.is_nan()) {
                f64::NAN
            } else {
                values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            }
        }
        "clip" => {
            if a.is_nan() || b.is_nan() || values[2].is_nan() {
                f64::NAN
            } else {
                a.max(b).min(values[2])
            }
        }
        "where" => {
            if a != 0.0 {
                b
            } else {
                values[2]
            }
        }
        "power" | "pow" => a.powf(b),
        "hypot" => a.hypot(b),
        "mod" | "remainder" => remainder(a, b),
        "fmod" => a % b,
        "degrees" | "rad2deg" => a.to_degrees(),
        "radians" | "deg2rad" => a.to_radians(),
        "sinc" => {
            if a == 0.0 {
                1.0
            } else {
                let v = std::f64::consts::PI * a;
                v.sin() / v
            }
        }
        "heaviside" => {
            if a.is_nan() {
                a
            } else if a < 0.0 {
                0.0
            } else if a > 0.0 {
                1.0
            } else {
                b
            }
        }
        "isnan" => boolean(a.is_nan()),
        "isinf" => boolean(a.is_infinite()),
        "isfinite" => boolean(a.is_finite()),
        "logical_not" => boolean(a == 0.0),
        "logical_and" => boolean(a != 0.0 && b != 0.0),
        "logical_or" => boolean(a != 0.0 || b != 0.0),
        "logical_xor" => boolean((a != 0.0) != (b != 0.0)),
        _ => unreachable!("function names checked by parser"),
    }
}

pub fn split_formulas(text: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                let value = text[start..i].trim();
                if !value.is_empty() {
                    result.push(value);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    let value = text[start..].trim();
    if !value.is_empty() {
        result.push(value);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn value(text: &str, x: f64) -> f64 {
        Expression::parse(text).unwrap().evaluate(x)
    }
    #[test]
    fn precedence_matches_python_and_numpy() {
        for (text, expected) in [
            ("-2^2", -4.0),
            ("2**3**2", 512.0),
            ("2^-2", 0.25),
            ("1+2*3", 7.0),
            ("(-2)^2", 4.0),
            ("-5%3", 1.0),
            ("5%-3", -1.0),
            ("-5//3", -2.0),
        ] {
            assert_eq!(value(text, 0.0), expected, "{text}");
        }
    }
    #[test]
    fn old_formula_syntax_and_constants() {
        assert!((value("y = 2x + sin(pi/2) + e^0", 3.0) - 8.0).abs() < 1e-12);
        assert_eq!(value("2(x+1)", 2.0), 6.0);
        assert_eq!(value("1e-3*x", 2000.0), 2.0);
    }
    #[test]
    fn nested_function_commas_stay_in_one_formula() {
        assert_eq!(
            split_formulas("sin(x), clip(x, -1, 1), where(x>0, x, 0)"),
            vec!["sin(x)", "clip(x, -1, 1)", "where(x>0, x, 0)"]
        );
    }
    #[test]
    fn multiargument_math_and_numpy_aliases() {
        assert_eq!(value("np.clip(x,-1,1)", 2.0), 1.0);
        assert_eq!(value("where(x>=0,sqrt(x),0)", -2.0), 0.0);
        assert_eq!(value("minimum(x,2)+hypot(3,4)", 4.0), 7.0);
    }
    #[test]
    fn numpy_rounding_sign_and_remainder() {
        assert_eq!(value("round(2.5)", 0.0), 2.0);
        assert_eq!(value("round(3.5)", 0.0), 4.0);
        assert_eq!(value("sign(0)", 0.0), 0.0);
        assert_eq!(value("mod(-5,3)", 0.0), 1.0);
    }
    #[test]
    fn domain_errors_are_non_finite_values() {
        assert!(value("sqrt(x)", -1.0).is_nan());
        assert!(value("1/x", 0.0).is_infinite());
        assert!(value("log(x)", -1.0).is_nan());
    }
    #[test]
    fn rejects_bad_names_arity_and_syntax_without_panics() {
        for text in [
            "",
            "sin()",
            "sin(1,2)",
            "unknown(x)",
            "x+",
            "(x",
            "x)",
            "2..3",
            "__import__('os')",
            "x[0]",
            "clip(x,1)",
        ] {
            assert!(Expression::parse(text).is_err(), "{text}");
        }
    }
    #[test]
    fn constants_work_as_flat_curves() {
        assert_eq!(value("5", -20.0), 5.0);
        assert_eq!(value("3.5", 20.0), 3.5);
    }
    #[test]
    fn scientific_notation_does_not_confuse_euler_constant() {
        assert_eq!(value("1e3+2E-2", 0.0), 1000.02);
        assert!((value("2e", 0.0) - 2.0 * std::f64::consts::E).abs() < 1e-12);
    }
    #[test]
    fn deeply_nested_and_oversized_input_is_rejected() {
        assert!(Expression::parse(&format!("{}x{}", "(".repeat(200), ")".repeat(200))).is_err());
        assert!(Expression::parse(&"x+".repeat(10_000)).is_err());
    }
}
