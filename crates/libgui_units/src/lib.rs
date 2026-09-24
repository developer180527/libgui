//! Numbers typed as text: `25.4mm`, `3/8"`, `w/2 + 1cm`.
//!
//! **Not part of libgui.** An expression language is an application's policy
//! — which functions exist, what a bare number means, how names resolve, and
//! whether the text or the number is the truth — so it lives here, beside
//! libgui, the way key bindings live in `libgui_keymap`. libgui's own field is
//! [`Ui::validated_input`], which asks the app. This crate is one answer to
//! give it, for an app that has none of its own.
//!
//! A dimension box in a CAD tool is a text field, not a slider, and what goes
//! into it is an expression in units. This is the evaluator behind
//! [`number_input`], kept separate and pure so it
//! can be tested without a frame and used by an app on its own — a command
//! line, a table cell, a script.
//!
//! **Units are the app's.** libgui ships two tables as a convenience,
//! [`Units::length_mm`] and [`Units::angle_deg`], because they are data rather
//! than policy; an app with a document unit of inches, or a unit of its own,
//! builds one with [`Units::new`] and [`Units::with`].
//!
//! The rules, which are the ones a machinist expects rather than the ones a
//! type checker would:
//!
//! - A number with no unit is in the field's display unit. `12` in a field
//!   showing millimetres is 12 mm.
//! - `+` and `-` between a bare number and a length treat the bare number the
//!   same way, so `10mm + 2` is 12 mm and `1in + 2` is `1in + 2mm`.
//! - `*` and `/` combine dimensions honestly: `w * 2` is a length, `w / h` is
//!   a ratio, and `w * h` is an area — which a length field refuses, rather
//!   than silently taking the number.
//! - A unit binds to the number just before it, except in the imperial
//!   fraction: `3/8"` is three eighths of an inch, not three divided by eight
//!   inches. That exception applies only when both sides of the `/` are plain
//!   numbers, which is the only time the other reading is nonsense.
//!
//! No functions (`sin`, `sqrt`) and no implicit multiplication: `2w` is an
//! error, not twice `w`. Both are easy to add and hard to take back.

use libgui::{FieldError, Ui, ValidatedResponse};
use std::fmt;

/// A named quantity an expression may refer to: `w`, `thickness`, `pitch`.
///
/// `value` is in the table's **base** unit, and `dim` says what it is: 1 for a
/// quantity of the field's kind (a length, in a length field), 0 for a plain
/// ratio or count. A count declared as a length would turn `n * 2` into a
/// length and quietly pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Var<'a> {
    pub name: &'a str,
    pub value: f64,
    pub dim: i32,
}

impl<'a> Var<'a> {
    /// A quantity of the field's own kind, in base units.
    pub fn quantity(name: &'a str, value: f64) -> Self {
        Self { name, value, dim: 1 }
    }

    /// A plain number: a count, a ratio, a scale factor.
    pub fn scalar(name: &'a str, value: f64) -> Self {
        Self { name, value, dim: 0 }
    }
}

/// What went wrong, and where. `at` is a byte offset into the text, so a field
/// can put a mark under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExprError {
    pub at: usize,
    pub message: String,
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExprError {}

/// The units a field understands, and the one it shows.
///
/// Every unit is a factor to one **base** unit. Values leave the evaluator in
/// base units, so an app stores millimetres (or metres, or whatever it chose)
/// whatever the user typed, and changes the display unit without touching its
/// model.
#[derive(Clone, Debug, PartialEq)]
pub struct Units {
    /// `(name, how many base units one of this is)`.
    units: Vec<(String, f64)>,
    /// Index into `units` of the one values are shown in, and the one a bare
    /// number means. `None` for a dimensionless field.
    display: Option<usize>,
}

impl Default for Units {
    fn default() -> Self {
        Self::none()
    }
}

impl Units {
    /// A field for plain numbers: arithmetic, no units at all.
    pub fn none() -> Self {
        Self { units: Vec::new(), display: None }
    }

    /// A table whose base unit is `base`. It is also the display unit until
    /// [`Units::display`] says otherwise.
    pub fn new(base: &str) -> Self {
        Self { units: vec![(base.to_string(), 1.0)], display: Some(0) }
    }

    /// Add a unit: one `name` is `factor` base units. The same factor under a
    /// second name is an alias — `in` and `"`.
    ///
    /// A non-finite or non-positive factor is ignored rather than stored,
    /// because it would turn every value typed in that unit into NaN or zero.
    pub fn with(mut self, name: &str, factor: f64) -> Self {
        if factor.is_finite() && factor > 0.0 && !name.is_empty() {
            if let Some(u) = self.units.iter_mut().find(|u| u.0 == name) {
                u.1 = factor;
            } else {
                self.units.push((name.to_string(), factor));
            }
            if self.display.is_none() {
                self.display = Some(self.units.len() - 1);
            }
        }
        self
    }

    /// Show values in `name`, and read bare numbers as `name`. Unknown names
    /// are ignored.
    pub fn display(mut self, name: &str) -> Self {
        if let Some(i) = self.units.iter().position(|u| u.0 == name) {
            self.display = Some(i);
        }
        self
    }

    /// Lengths with a base of millimetres: `mm`, `cm`, `m`, `um`/`µm`, `in`/`"`,
    /// `ft`/`'`. Shown in millimetres.
    pub fn length_mm() -> Self {
        Self::new("mm")
            .with("cm", 10.0)
            .with("m", 1000.0)
            .with("um", 0.001)
            .with("µm", 0.001)
            .with("in", 25.4)
            .with("\"", 25.4)
            .with("ft", 304.8)
            .with("'", 304.8)
    }

    /// Angles with a base of degrees: `deg`/`°`, `rad`. Shown in degrees.
    pub fn angle_deg() -> Self {
        Self::new("deg").with("°", 1.0).with("rad", 180.0 / std::f64::consts::PI).display("°")
    }

    /// The display unit's name, or `""` for a dimensionless field.
    pub fn display_name(&self) -> &str {
        self.display.map(|i| self.units[i].0.as_str()).unwrap_or("")
    }

    fn display_factor(&self) -> f64 {
        self.display.map(|i| self.units[i].1).unwrap_or(1.0)
    }

    fn lookup(&self, name: &str) -> Option<f64> {
        self.units.iter().find(|u| u.0 == name).map(|u| u.1)
    }

    fn has_units(&self) -> bool {
        self.display.is_some()
    }

    /// `value` (in base units) as the field shows it: in the display unit, at
    /// most `decimals` places, trailing zeros trimmed. `25.4` → `"25.4 mm"`.
    pub fn format(&self, value: f64, decimals: u32) -> String {
        if !value.is_finite() {
            return String::new();
        }
        let shown = value / self.display_factor();
        let mut s = format!("{:.*}", decimals.min(12) as usize, shown);
        if s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        // Rounding can leave a negative zero, which reads as a sign error.
        if s == "-0" {
            s = "0".to_string();
        }
        let name = self.display_name();
        if !name.is_empty() {
            // A symbol hugs its number, as it is written: 12", 45°, 3'.
            if name.chars().count() > 1 || name.chars().next().is_some_and(|c| c.is_alphanumeric()) {
                s.push(' ');
            }
            s.push_str(name);
        }
        s
    }

    /// Evaluate `text` to a value in base units.
    pub fn eval(&self, text: &str, vars: &[Var]) -> Result<f64, ExprError> {
        let tokens = lex(text, self)?;
        let mut p = Parser { tokens: &tokens, i: 0, units: self, vars, end: text.len() };
        if p.tokens.is_empty() {
            return Err(err(0, "enter a value"));
        }
        let v = p.expr()?;
        if let Some(t) = p.peek() {
            let message = match &t.tok {
                // `2mm mm`: the second unit has nothing left to bind to.
                Tok::Ident(n) if self.lookup(n).is_some() => "this already has a unit".to_string(),
                // `2w`: implicit multiplication, which is deliberately absent.
                Tok::Ident(n) => format!("'{n}' is not understood here: use * to multiply"),
                Tok::Num(_) => "a number cannot follow a number: use an operator".to_string(),
                Tok::Op(')') => "this ')' has no '('".to_string(),
                Tok::Op(_) => "unexpected text here".to_string(),
            };
            return Err(ExprError { at: t.at, message });
        }
        let value = match (v.dim, self.has_units()) {
            (_, false) => v.value,
            // A bare result is in the display unit, like a bare number.
            (0, true) => v.value * self.display_factor(),
            (1, true) => v.value,
            (_, true) => {
                let want = self.display_name();
                return Err(err(0, &format!("this is not in {want}: check for a unit multiplied by a unit")));
            }
        };
        if !value.is_finite() {
            return Err(err(0, "the result is too large"));
        }
        Ok(value)
    }
}

fn err(at: usize, message: &str) -> ExprError {
    ExprError { at, message: message.to_string() }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    at: usize,
}

/// Characters that are a whole unit on their own, and cannot start a name.
fn is_symbol_unit(c: char) -> bool {
    matches!(c, '"' | '\'' | '°' | '″' | '′')
}

fn lex(text: &str, units: &Units) -> Result<Vec<Token>, ExprError> {
    let mut out = Vec::new();
    let mut it = text.char_indices().peekable();
    while let Some(&(at, c)) = it.peek() {
        if c.is_whitespace() {
            it.next();
        } else if c.is_ascii_digit() || c == '.' {
            let mut end = at;
            let mut dots = 0;
            while let Some(&(i, d)) = it.peek() {
                if d.is_ascii_digit() || (d == '.' && dots == 0) {
                    dots += (d == '.') as i32;
                    end = i + d.len_utf8();
                    it.next();
                } else {
                    break;
                }
            }
            let s = &text[at..end];
            let v: f64 = s.parse().map_err(|_| err(at, "not a number"))?;
            out.push(Token { tok: Tok::Num(v), at });
        } else if matches!(c, '+' | '-' | '*' | '/' | '(' | ')' | '×' | '÷') {
            let op = match c {
                '×' => '*',
                '÷' => '/',
                c => c,
            };
            out.push(Token { tok: Tok::Op(op), at });
            it.next();
        } else if is_symbol_unit(c) {
            // Typographic primes read as the ASCII marks they stand for.
            let name = match c {
                '″' => '"',
                '′' => '\'',
                c => c,
            };
            out.push(Token { tok: Tok::Ident(name.to_string()), at });
            it.next();
        } else if c.is_alphabetic() || c == '_' {
            let mut end = at;
            while let Some(&(i, d)) = it.peek() {
                if d.is_alphanumeric() || d == '_' {
                    end = i + d.len_utf8();
                    it.next();
                } else {
                    break;
                }
            }
            out.push(Token { tok: Tok::Ident(text[at..end].to_string()), at });
        } else if units.lookup(&c.to_string()).is_some() {
            // An app's own one-character symbol unit.
            out.push(Token { tok: Tok::Ident(c.to_string()), at });
            it.next();
        } else {
            return Err(err(at, &format!("'{c}' is not understood here")));
        }
    }
    Ok(out)
}

/// A value on its way through the parser.
#[derive(Clone, Copy, Debug)]
struct Val {
    /// In base units when `dim` is 1; a plain number when 0.
    value: f64,
    /// Powers of the field's quantity: 0 a ratio, 1 a length, 2 an area.
    dim: i32,
    /// Written as a plain number, possibly negated, and nothing else: the
    /// only thing an imperial fraction's numerator and denominator can be.
    literal: bool,
}

struct Parser<'a> {
    tokens: &'a [Token],
    i: usize,
    units: &'a Units,
    vars: &'a [Var<'a>],
    end: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.i)
    }

    fn at(&self) -> usize {
        self.peek().map(|t| t.at).unwrap_or(self.end)
    }

    fn eat_op(&mut self, ops: &[char]) -> Option<char> {
        match self.peek() {
            Some(Token { tok: Tok::Op(c), .. }) if ops.contains(c) => {
                let c = *c;
                self.i += 1;
                Some(c)
            }
            _ => None,
        }
    }

    /// A unit name at the cursor, if there is one.
    fn unit_here(&self) -> Option<f64> {
        match self.peek() {
            Some(Token { tok: Tok::Ident(n), .. }) if self.units.has_units() => self.units.lookup(n),
            _ => None,
        }
    }

    /// Apply a unit following `v`, if one does.
    fn suffix(&mut self, v: Val) -> Result<Val, ExprError> {
        let at = self.at();
        let Some(f) = self.unit_here() else { return Ok(v) };
        self.i += 1;
        if v.dim != 0 {
            return Err(err(at, "this already has a unit"));
        }
        Ok(Val { value: v.value * f, dim: 1, literal: false })
    }

    fn expr(&mut self) -> Result<Val, ExprError> {
        let mut acc = self.term()?;
        while let Some(op) = self.eat_op(&['+', '-']) {
            let at = self.at();
            let rhs = self.term()?;
            let (a, b) = self.align(acc, rhs, at)?;
            let value = if op == '+' { a.value + b.value } else { a.value - b.value };
            acc = Val { value, dim: a.dim, literal: false };
        }
        Ok(acc)
    }

    /// Bring two operands of `+`/`-` to the same dimension, promoting a bare
    /// number to the display unit when the other side is a quantity.
    fn align(&self, a: Val, b: Val, at: usize) -> Result<(Val, Val), ExprError> {
        let f = self.units.display_factor();
        match (a.dim, b.dim) {
            (x, y) if x == y => Ok((a, b)),
            (0, 1) => Ok((Val { value: a.value * f, dim: 1, ..a }, b)),
            (1, 0) => Ok((a, Val { value: b.value * f, dim: 1, ..b })),
            _ => Err(err(at, "these cannot be added: their units differ")),
        }
    }

    fn term(&mut self) -> Result<Val, ExprError> {
        let first = self.unary()?;
        let mut acc = self.suffix(first)?;
        while let Some(op) = self.eat_op(&['*', '/']) {
            let at = self.at();
            let rhs = self.unary()?;
            // 3/8" -- the fraction takes the unit, not the denominator.
            if op == '/' && acc.literal && rhs.literal && self.unit_here().is_some() {
                if rhs.value == 0.0 {
                    return Err(err(at, "division by zero"));
                }
                let q = Val { value: acc.value / rhs.value, dim: 0, literal: false };
                acc = self.suffix(q)?;
                continue;
            }
            let rhs = self.suffix(rhs)?;
            acc = if op == '*' {
                Val { value: acc.value * rhs.value, dim: acc.dim + rhs.dim, literal: false }
            } else {
                if rhs.value == 0.0 {
                    return Err(err(at, "division by zero"));
                }
                Val { value: acc.value / rhs.value, dim: acc.dim - rhs.dim, literal: false }
            };
        }
        Ok(acc)
    }

    fn unary(&mut self) -> Result<Val, ExprError> {
        match self.eat_op(&['-', '+']) {
            Some('-') => {
                let v = self.unary()?;
                Ok(Val { value: -v.value, ..v })
            }
            Some(_) => self.unary(),
            None => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Val, ExprError> {
        let at = self.at();
        let Some(t) = self.peek().cloned() else {
            return Err(err(at, "a value is missing here"));
        };
        self.i += 1;
        match t.tok {
            Tok::Num(v) => Ok(Val { value: v, dim: 0, literal: true }),
            Tok::Op('(') => {
                let v = self.expr()?;
                if self.eat_op(&[')']).is_none() {
                    return Err(err(self.at(), "a ')' is missing"));
                }
                Ok(Val { literal: false, ..v })
            }
            Tok::Op(_) => Err(err(at, "a value is missing here")),
            Tok::Ident(name) => {
                if let Some(v) = self.vars.iter().find(|v| v.name == name) {
                    // A variable of the field's kind is meaningless in a field
                    // with no units; treat it as the number it is.
                    let dim = if self.units.has_units() { v.dim } else { 0 };
                    return Ok(Val { value: v.value, dim, literal: false });
                }
                if name == "pi" || name == "π" {
                    return Ok(Val { value: std::f64::consts::PI, dim: 0, literal: false });
                }
                if self.units.has_units() && self.units.lookup(&name).is_some() {
                    return Err(err(at, &format!("'{name}' needs a number before it")));
                }
                Err(err(at, &format!("'{name}' is not a unit or a name this field knows")))
            }
        }
    }
}

/// How [`number_input`] behaves beyond its units.
#[derive(Clone, Copy, Debug)]
pub struct NumberOptions<'a> {
    /// Places shown when the field is not being edited. Trailing zeros are
    /// trimmed, so this is a maximum.
    pub decimals: u32,
    /// Smallest acceptable value, in base units. Outside the range is
    /// **refused with a message**, not clamped.
    pub min: f64,
    pub max: f64,
    /// Names an expression may use.
    pub vars: &'a [Var<'a>],
}

impl Default for NumberOptions<'_> {
    fn default() -> Self {
        Self { decimals: 3, min: f64::NEG_INFINITY, max: f64::INFINITY, vars: &[] }
    }
}

/// What [`number_input`] did this frame.
#[derive(Clone, Debug, Default)]
pub struct NumberResponse {
    pub field: ValidatedResponse,
    /// A new value was written this frame.
    pub committed: bool,
}

impl From<ExprError> for FieldError {
    fn from(e: ExprError) -> Self {
        FieldError::new(e.message).at(e.at)
    }
}

/// A number typed as an expression in these units, for an app that has no
/// grammar of its own. `value` is in base units and changes only on commit.
///
/// This is [`Ui::validated_input`] with [`Units::eval`] as the validator, and
/// nothing more. It keeps a **number**; an app whose source of truth is the
/// expression text — a parametric model — uses `validated_input` directly with
/// its own evaluator, and keeps the text.
pub fn number_input(ui: &mut Ui, key: &str, value: &mut f64, units: &Units, opts: &NumberOptions) -> NumberResponse {
    let mut text = units.format(*value, opts.decimals);
    let mut parsed = None;
    let field = ui.validated_input(key, &mut text, |t| {
        let v = units.eval(t, opts.vars)?;
        if v < opts.min {
            return Err(FieldError::new(format!("must be at least {}", units.format(opts.min, opts.decimals))));
        }
        if v > opts.max {
            return Err(FieldError::new(format!("must be at most {}", units.format(opts.max, opts.decimals))));
        }
        parsed = Some(v);
        Ok(())
    });
    // Accepted text may be new while the number is not (`25.4mm` for
    // `25.4 mm`); only a new number is a change.
    let committed = matches!(parsed, Some(v) if v != *value);
    if let Some(v) = parsed {
        *value = v;
    }
    NumberResponse { field, committed }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm(s: &str) -> Result<f64, ExprError> {
        Units::length_mm().eval(s, &[])
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_bare_number_is_in_the_display_unit() {
        assert_eq!(mm("12").unwrap(), 12.0);
        let inches = Units::length_mm().display("in");
        assert!(close(inches.eval("2", &[]).unwrap(), 50.8));
    }

    #[test]
    fn units_convert_to_the_base() {
        assert!(close(mm("25.4mm").unwrap(), 25.4));
        assert!(close(mm("1in").unwrap(), 25.4));
        assert!(close(mm("1 in").unwrap(), 25.4));
        assert!(close(mm("2cm").unwrap(), 20.0));
        assert!(close(mm("1'").unwrap(), 304.8));
        assert!(close(mm("1ft + 1\"").unwrap(), 330.2));
    }

    #[test]
    fn a_bare_number_beside_a_length_is_in_the_display_unit() {
        assert!(close(mm("10mm + 2").unwrap(), 12.0));
        assert!(close(mm("1in + 2").unwrap(), 27.4));
        assert!(close(mm("2 + 1in").unwrap(), 27.4));
    }

    #[test]
    fn an_imperial_fraction_takes_the_unit() {
        assert!(close(mm("3/8\"").unwrap(), 25.4 * 3.0 / 8.0));
        assert!(close(mm("3/8 in").unwrap(), 25.4 * 3.0 / 8.0));
        assert!(close(mm("-3/8\"").unwrap(), -25.4 * 3.0 / 8.0));
        // Only between plain numbers: a ratio of lengths is still a ratio.
        assert!(close(mm("10mm/2mm").unwrap(), 5.0), "10mm/2mm is a ratio of 5, shown in mm");
    }

    #[test]
    fn dimensions_are_checked_not_ignored() {
        let e = mm("2mm * 3mm").unwrap_err();
        assert!(e.message.contains("not in mm"), "{e}");
        assert!(mm("2mm * 3").is_ok());
        let e = mm("1mm + 1mm*1mm").unwrap_err();
        assert!(e.message.contains("cannot be added"), "{e}");
        let e = mm("2mm mm").unwrap_err();
        assert!(e.message.contains("already has a unit"), "{e}");
    }

    #[test]
    fn variables_carry_their_kind() {
        let vars = [Var::quantity("w", 40.0), Var::scalar("n", 4.0)];
        let u = Units::length_mm();
        assert!(close(u.eval("w/2", &vars).unwrap(), 20.0));
        assert!(close(u.eval("w/n + 1cm", &vars).unwrap(), 20.0));
        assert!(close(u.eval("(w - 5) * 2", &vars).unwrap(), 70.0));
        // w*w is an area, not a length.
        assert!(u.eval("w*w", &vars).is_err());
    }

    #[test]
    fn precedence_and_parentheses() {
        let u = Units::none();
        assert_eq!(u.eval("1 + 2 * 3", &[]).unwrap(), 7.0);
        assert_eq!(u.eval("(1 + 2) * 3", &[]).unwrap(), 9.0);
        assert_eq!(u.eval("-2 * -3", &[]).unwrap(), 6.0);
        assert_eq!(u.eval("8 / 2 / 2", &[]).unwrap(), 2.0);
        assert_eq!(u.eval("6 × 7", &[]).unwrap(), 42.0);
    }

    #[test]
    fn errors_say_where() {
        let e = mm("12 +").unwrap_err();
        assert_eq!(e.at, 4);
        let e = mm("12 $").unwrap_err();
        assert_eq!(e.at, 3);
        let e = mm("(1 + 2").unwrap_err();
        assert!(e.message.contains("')'"), "{e}");
        let e = mm("2w").unwrap_err();
        assert!(e.message.contains("'w'"), "{e}");
        let e = mm("mm").unwrap_err();
        assert!(e.message.contains("needs a number"), "{e}");
        assert!(mm("").is_err());
        assert!(mm("   ").is_err());
        assert!(mm("1/0").unwrap_err().message.contains("zero"));
        assert!(mm("1..2").is_err());
    }

    #[test]
    fn a_unitless_field_does_arithmetic_only() {
        let u = Units::none();
        assert_eq!(u.eval("2 + 3", &[]).unwrap(), 5.0);
        assert!(u.eval("2mm", &[]).is_err(), "a unitless field has no mm");
    }

    #[test]
    fn angles() {
        let u = Units::angle_deg();
        assert!(close(u.eval("45", &[]).unwrap(), 45.0));
        assert!(close(u.eval("pi rad", &[]).unwrap(), 180.0));
        assert!(close(u.eval("pi*1rad", &[]).unwrap(), 180.0));
        assert!(close(u.eval("90°", &[]).unwrap(), 90.0));
    }

    #[test]
    fn formatting() {
        let u = Units::length_mm();
        assert_eq!(u.format(25.4, 3), "25.4 mm");
        assert_eq!(u.format(12.0, 3), "12 mm");
        assert_eq!(u.format(-0.0001, 2), "0 mm");
        assert_eq!(u.clone().display("in").format(25.4, 3), "1 in");
        assert_eq!(u.display("\"").format(25.4, 3), "1\"");
        assert_eq!(Units::angle_deg().format(45.0, 1), "45°");
        assert_eq!(Units::none().format(1.5, 2), "1.5");
        assert_eq!(Units::none().format(f64::NAN, 2), "");
    }

    #[test]
    fn formatting_round_trips() {
        let u = Units::length_mm();
        for v in [0.0, 1.0, 25.4, -3.175, 1234.5678, 0.001] {
            let s = u.format(v, 4);
            assert!(close(u.eval(&s, &[]).unwrap(), (v * 1e4).round() / 1e4), "{v} → {s}");
        }
        let inch = u.display("\"");
        let s = inch.format(9.525, 4);
        assert!(close(inch.eval(&s, &[]).unwrap(), 9.525), "{s}");
    }

    #[test]
    fn bad_unit_factors_are_refused() {
        let u = Units::new("mm").with("bad", f64::NAN).with("zero", 0.0).with("neg", -1.0);
        assert!(u.eval("1bad", &[]).is_err());
        assert!(u.eval("1zero", &[]).is_err());
        assert!(u.eval("1neg", &[]).is_err());
    }
}
