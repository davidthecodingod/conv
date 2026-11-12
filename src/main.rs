use std::env;
use std::io::{self, BufRead, IsTerminal, Write};
use std::process;

// ============================================================
// Scanner: text -> tokens
// ============================================================

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Word(String),
    Comma,
}

struct Scanner {
    source: Vec<char>,
    tokens: Vec<Token>,
    start: usize,   // first char of the lexeme being scanned
    current: usize, // char we're about to consume
}

impl Scanner {
    fn new(source: &str) -> Self {
        Scanner {
            source: source.chars().collect(),
            tokens: Vec::new(),
            start: 0,
            current: 0,
        }
    }

    fn scan_tokens(mut self) -> Result<Vec<Token>, String> {
        while !self.is_at_end() {
            // We are at the beginning of the next lexeme.
            self.start = self.current;
            self.scan_token()?;
        }
        Ok(self.tokens)
    }

    fn scan_token(&mut self) -> Result<(), String> {
        let c = self.advance();
        match c {
            ',' => self.add_token(Token::Comma),
            ' ' | '\r' | '\t' | '\n' => {} // ignore whitespace

            // a '-' only starts a number if a digit follows it
            '-' if is_digit(self.peek()) => self.number()?,

            c if is_digit(c) => self.number()?,
            c if is_word_start(c) => self.word(),

            _ => return Err(format!("unexpected character '{c}'")),
        }
        Ok(())
    }

    fn number(&mut self) -> Result<(), String> {
        while is_digit(self.peek()) {
            self.advance();
        }

        // Look for a fractional part.
        if self.peek() == '.' && is_digit(self.peek_next()) {
            self.advance(); // consume the "."
            while is_digit(self.peek()) {
                self.advance();
            }
        }

        let text = self.lexeme();
        let value: f64 = text.parse().map_err(|_| format!("bad number '{text}'"))?;
        self.add_token(Token::Number(value));
        Ok(())
    }

    fn word(&mut self) {
        while is_word_char(self.peek()) {
            self.advance();
        }
        let text = self.lexeme();
        self.add_token(Token::Word(text));
    }

    // ---------- helpers ----------

    fn is_at_end(&self) -> bool {
        self.current >= self.source.len()
    }

    fn advance(&mut self) -> char {
        let c = self.source[self.current];
        self.current += 1;
        c
    }

    fn peek(&self) -> char {
        if self.is_at_end() {
            return '\0';
        }
        self.source[self.current]
    }

    fn peek_next(&self) -> char {
        if self.current + 1 >= self.source.len() {
            return '\0';
        }
        self.source[self.current + 1]
    }

    fn lexeme(&self) -> String {
        self.source[self.start..self.current].iter().collect()
    }

    fn add_token(&mut self, token: Token) {
        self.tokens.push(token);
    }
}

fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}

fn is_word_start(c: char) -> bool {
    c.is_alphabetic() || c == '°'
}

// units like m/s, ft^2, m², °F, and m2 are single words
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '°' | '²' | '/' | '^')
}

// ============================================================
// Units: what the parser produces
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq)]
enum Category {
    Length,
    Mass,
    Volume,
    Temperature,
    Speed,
    Time,
    Data,
    Area,
}

impl Category {
    fn name(self) -> &'static str {
        match self {
            Category::Length => "length",
            Category::Mass => "mass",
            Category::Volume => "volume",
            Category::Temperature => "temperature",
            Category::Speed => "speed",
            Category::Time => "time",
            Category::Data => "data",
            Category::Area => "area",
        }
    }
}

/// Every unit converts through a base unit (meter, gram, liter, kelvin, ...):
///   base  = (value + offset) * factor
///   value = base / factor - offset
/// Only temperature has a non-zero offset.
#[derive(Debug, Clone, Copy)]
struct Unit {
    name: &'static str,
    category: Category,
    factor: f64,
    offset: f64,
}

#[derive(Debug)]
struct Measure {
    value: f64,
    unit: Unit,
}

#[derive(Debug)]
struct Conversion {
    source: Vec<Measure>, // "5 ft 11 in" is two measures, summed later
    targets: Vec<Unit>,
}

/// Every spelling we accept (lowercase). Returns (display name, category, factor, offset).
fn find_unit(name: &str) -> Option<Unit> {
    use Category::*;
    let (name, category, factor, offset) = match name.to_lowercase().as_str() {
        // Length (base: meter)
        "mm" | "millimeter" | "millimeters" | "millimetre" | "millimetres" => ("mm", Length, 0.001, 0.0),
        "cm" | "centimeter" | "centimeters" | "centimetre" | "centimetres" => ("cm", Length, 0.01, 0.0),
        "m" | "meter" | "meters" | "metre" | "metres" => ("m", Length, 1.0, 0.0),
        "km" | "kilometer" | "kilometers" | "kilometre" | "kilometres" => ("km", Length, 1000.0, 0.0),
        "in" | "inch" | "inches" => ("in", Length, 0.0254, 0.0),
        "ft" | "foot" | "feet" => ("ft", Length, 0.3048, 0.0),
        "yd" | "yard" | "yards" => ("yd", Length, 0.9144, 0.0),
        "mi" | "mile" | "miles" => ("mi", Length, 1609.344, 0.0),
        "nmi" | "nautical mile" | "nautical miles" => ("nmi", Length, 1852.0, 0.0),
        "furlong" | "furlongs" => ("furlong", Length, 201.168, 0.0),

        // Mass (base: gram)
        "mg" | "milligram" | "milligrams" => ("mg", Mass, 0.001, 0.0),
        "g" | "gram" | "grams" => ("g", Mass, 1.0, 0.0),
        "kg" | "kilogram" | "kilograms" | "kilo" | "kilos" => ("kg", Mass, 1000.0, 0.0),
        "oz" | "ounce" | "ounces" => ("oz", Mass, 28.349523125, 0.0),
        "lb" | "lbs" | "pound" | "pounds" => ("lb", Mass, 453.59237, 0.0),
        "st" | "stone" | "stones" => ("st", Mass, 6350.29318, 0.0),
        "ton" | "tons" => ("ton", Mass, 907_184.74, 0.0), // US short ton
        "tonne" | "tonnes" => ("tonne", Mass, 1_000_000.0, 0.0),

        // Volume (base: liter)
        "ml" | "milliliter" | "milliliters" | "millilitre" | "millilitres" => ("ml", Volume, 0.001, 0.0),
        "l" | "liter" | "liters" | "litre" | "litres" => ("l", Volume, 1.0, 0.0),
        "tsp" | "teaspoon" | "teaspoons" => ("tsp", Volume, 0.00492892159375, 0.0),
        "tbsp" | "tablespoon" | "tablespoons" => ("tbsp", Volume, 0.01478676478125, 0.0),
        "fl oz" | "floz" | "fluid ounce" | "fluid ounces" => ("fl oz", Volume, 0.0295735295625, 0.0),
        "cup" | "cups" => ("cup", Volume, 0.2365882365, 0.0),
        "pt" | "pint" | "pints" => ("pt", Volume, 0.473176473, 0.0),
        "qt" | "quart" | "quarts" => ("qt", Volume, 0.946352946, 0.0),
        "gal" | "gallon" | "gallons" => ("gal", Volume, 3.785411784, 0.0),

        // Temperature (base: kelvin)
        "c" | "°c" | "celsius" | "degc" => ("°C", Temperature, 1.0, 273.15),
        "f" | "°f" | "fahrenheit" | "degf" => ("°F", Temperature, 5.0 / 9.0, 459.67),
        "k" | "kelvin" => ("K", Temperature, 1.0, 0.0),

        // Speed (base: m/s)
        "m/s" | "mps" => ("m/s", Speed, 1.0, 0.0),
        "km/h" | "kmh" | "kph" => ("km/h", Speed, 1.0 / 3.6, 0.0),
        "mph" | "mi/h" => ("mph", Speed, 0.44704, 0.0),
        "kn" | "knot" | "knots" | "kt" | "kts" => ("kn", Speed, 1852.0 / 3600.0, 0.0),

        // Time (base: second)
        "s" | "sec" | "secs" | "second" | "seconds" => ("s", Time, 1.0, 0.0),
        "min" | "mins" | "minute" | "minutes" => ("min", Time, 60.0, 0.0),
        "h" | "hr" | "hrs" | "hour" | "hours" => ("h", Time, 3600.0, 0.0),
        "d" | "day" | "days" => ("day", Time, 86_400.0, 0.0),
        "wk" | "week" | "weeks" => ("week", Time, 604_800.0, 0.0),

        // Data (base: byte)
        "b" | "byte" | "bytes" => ("B", Data, 1.0, 0.0),
        "kb" | "kilobyte" | "kilobytes" => ("KB", Data, 1e3, 0.0),
        "mb" | "megabyte" | "megabytes" => ("MB", Data, 1e6, 0.0),
        "gb" | "gigabyte" | "gigabytes" => ("GB", Data, 1e9, 0.0),
        "tb" | "terabyte" | "terabytes" => ("TB", Data, 1e12, 0.0),
        "kib" | "kibibyte" | "kibibytes" => ("KiB", Data, 1024.0, 0.0),
        "mib" | "mebibyte" | "mebibytes" => ("MiB", Data, 1_048_576.0, 0.0),
        "gib" | "gibibyte" | "gibibytes" => ("GiB", Data, 1_073_741_824.0, 0.0),
        "tib" | "tebibyte" | "tebibytes" => ("TiB", Data, 1_099_511_627_776.0, 0.0),

        // Area (base: m²)
        "m²" | "m2" | "m^2" | "sqm" | "square meter" | "square meters" => ("m²", Area, 1.0, 0.0),
        "km²" | "km2" | "km^2" | "sqkm" | "square kilometer" | "square kilometers" => ("km²", Area, 1e6, 0.0),
        "ft²" | "ft2" | "ft^2" | "sqft" | "square foot" | "square feet" => ("ft²", Area, 0.09290304, 0.0),
        "acre" | "acres" => ("acre", Area, 4046.8564224, 0.0),
        "ha" | "hectare" | "hectares" => ("ha", Area, 10_000.0, 0.0),

        _ => return None,
    };
    Some(Unit { name, category, factor, offset })
}

// ============================================================
// Parser: tokens -> Conversion
// ============================================================
//
// Grammar:
//   conversion → quantity SEPARATOR targets EOF ;
//   quantity   → ( NUMBER unit )+ ;
//   targets    → unit ( "," unit )* ;
//   unit       → WORD WORD? ;              // "fl oz", "nautical mile"
//   SEPARATOR  → "to" | "in" | "as" ;
//
// "in" is both a unit and a separator. The grammar resolves it with no
// special case: a unit is only parsed right after a NUMBER, so an "in" that
// isn't preceded by one is the separator.

struct Parser {
    tokens: Vec<Token>,
    current: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, current: 0 }
    }

    fn parse(&mut self) -> Result<Conversion, String> {
        let source = self.quantity()?;
        self.separator()?;
        let targets = self.targets()?;

        if let Some(extra) = self.peek() {
            return Err(format!("unexpected {} after the target unit", describe(extra)));
        }

        // Every target must be the same kind of thing as the source.
        let category = source[0].unit.category;
        for t in &targets {
            if t.category != category {
                return Err(format!(
                    "can't convert {} to {}",
                    category.name(),
                    t.category.name()
                ));
            }
        }
        Ok(Conversion { source, targets })
    }

    // quantity → ( NUMBER unit )+ ;
    fn quantity(&mut self) -> Result<Vec<Measure>, String> {
        let mut measures = Vec::new();

        while let Some(value) = self.match_number() {
            if !matches!(self.peek(), Some(Token::Word(_))) {
                return Err(format!("expected a unit after {}", fmt_num(value, 6)));
            }
            let unit = self.unit()?;
            measures.push(Measure { value, unit });
        }

        if measures.is_empty() {
            return Err(match self.peek() {
                Some(Token::Word(w)) => format!("expected a number before '{w}'"),
                _ => "expected a number, e.g. 10 km to mi".to_string(),
            });
        }

        let category = measures[0].unit.category;
        if measures.iter().any(|m| m.unit.category != category) {
            return Err("can't mix different kinds of units in one quantity".to_string());
        }
        if category == Category::Temperature && measures.len() > 1 {
            return Err("temperatures can't be added together".to_string());
        }
        Ok(measures)
    }

    // SEPARATOR → "to" | "in" | "as" ;
    fn separator(&mut self) -> Result<(), String> {
        if let Some(Token::Word(w)) = self.peek() {
            if matches!(w.to_lowercase().as_str(), "to" | "in" | "as") {
                self.advance();
                return Ok(());
            }
        }
        match self.peek() {
            Some(t) => Err(format!("expected 'to' but found {}", describe(t))),
            None => Err("expected 'to' and a target unit, e.g. 10 km to mi".to_string()),
        }
    }

    // targets → unit ( "," unit )* ;
    fn targets(&mut self) -> Result<Vec<Unit>, String> {
        let mut targets = vec![self.unit()?];
        while self.match_comma() {
            targets.push(self.unit()?);
        }
        Ok(targets)
    }

    // unit → WORD WORD? ;
    fn unit(&mut self) -> Result<Unit, String> {
        let first = match self.peek() {
            Some(Token::Word(w)) => w.clone(),
            _ => return Err("expected a unit".to_string()),
        };

        // Try two words first, so "fl oz" wins over "fl".
        if let Some(Token::Word(second)) = self.tokens.get(self.current + 1) {
            let joined = format!("{first} {second}");
            if let Some(unit) = find_unit(&joined) {
                self.current += 2;
                return Ok(unit);
            }
        }

        match find_unit(&first) {
            Some(unit) => {
                self.advance();
                Ok(unit)
            }
            None => Err(unknown_unit_error(&first)),
        }
    }

    // ---------- helpers ----------

    fn is_at_end(&self) -> bool {
        self.current >= self.tokens.len()
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.current)
    }

    fn advance(&mut self) {
        if !self.is_at_end() {
            self.current += 1;
        }
    }

    fn match_number(&mut self) -> Option<f64> {
        if let Some(Token::Number(n)) = self.peek() {
            let n = *n;
            self.advance();
            return Some(n);
        }
        None
    }

    fn match_comma(&mut self) -> bool {
        if self.peek() == Some(&Token::Comma) {
            self.advance();
            return true;
        }
        false
    }
}

fn describe(t: &Token) -> String {
    match t {
        Token::Number(n) => format!("number {}", fmt_num(*n, 6)),
        Token::Word(w) => format!("'{w}'"),
        Token::Comma => "','".to_string(),
    }
}

// ============================================================
// Errors: "did you mean"
// ============================================================

const SUGGESTIONS: &[&str] = &[
    "mm", "cm", "m", "km", "inch", "inches", "ft", "feet", "yd", "yard", "mi", "mile", "miles",
    "furlong", "mg", "g", "kg", "oz", "lb", "pound", "pounds", "stone", "ton", "tonne", "ml", "l",
    "liter", "tsp", "tbsp", "cup", "pint", "quart", "gallon", "gal", "celsius", "fahrenheit",
    "kelvin", "mph", "knot", "second", "minute", "hour", "day", "week", "byte", "kb", "mb", "gb",
    "tb", "kib", "mib", "gib", "acre", "hectare",
];

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

fn unknown_unit_error(word: &str) -> String {
    let lower = word.to_lowercase();
    let best = SUGGESTIONS
        .iter()
        .map(|s| (levenshtein(&lower, s), *s))
        .min_by_key(|(d, _)| *d);

    match best {
        Some((d, name)) if d <= 2 && d < lower.chars().count() => {
            format!("unknown unit '{word}'. Did you mean '{name}'?")
        }
        _ => format!("unknown unit '{word}'"),
    }
}

// ============================================================
// Evaluate + format
// ============================================================

fn fmt_num(x: f64, precision: usize) -> String {
    let s = format!("{x:.precision$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    };
    if s == "-0" {
        "0".to_string()
    } else {
        s
    }
}

fn evaluate(conversion: &Conversion, precision: usize) -> String {
    let base: f64 = conversion
        .source
        .iter()
        .map(|m| (m.value + m.unit.offset) * m.unit.factor)
        .sum();

    let label = conversion
        .source
        .iter()
        .map(|m| format!("{} {}", fmt_num(m.value, 6), m.unit.name))
        .collect::<Vec<_>>()
        .join(" ");

    let results = conversion
        .targets
        .iter()
        .map(|t| format!("{} {}", fmt_num(base / t.factor - t.offset, precision), t.name))
        .collect::<Vec<_>>()
        .join(", ");

    format!("{label} = {results}")
}

fn run(source: &str, precision: usize) -> Result<String, String> {
    let tokens = Scanner::new(source).scan_tokens()?;
    let conversion = Parser::new(tokens).parse()?;
    Ok(evaluate(&conversion, precision))
}

// ============================================================
// CLI: one-shot with args, or a REPL with none
// ============================================================

const USAGE: &str = "\
conv - natural-syntax unit converter

usage:
  conv [--precision N] <quantity> to <unit>[, <unit>...]
  conv                      start an interactive session

examples:
  conv 10 km to mi
  conv 5 ft 11 in to cm
  conv 72 f to c
  conv 1 km to m, ft

options:
  -p, --precision N   decimal places (default 4)
  -h, --help          show this help
  -V, --version       show the version";

fn repl(precision: usize) {
    let interactive = io::stdin().is_terminal();
    if interactive {
        println!("conv {} - type a conversion, 'help', or 'exit'", env!("CARGO_PKG_VERSION"));
    }

    let stdin = io::stdin();
    loop {
        if interactive {
            print!("> ");
            io::stdout().flush().ok();
        }

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // EOF (Ctrl-D)
            Ok(_) => {}
            Err(e) => {
                eprintln!("error: {e}");
                break;
            }
        }

        let line = line.trim();
        match line {
            "" => continue,
            "exit" | "quit" | "q" => break,
            "help" | "?" => println!("{USAGE}"),
            _ => match run(line, precision) {
                Ok(out) => println!("{out}"),
                Err(e) => eprintln!("error: {e}"),
            },
        }
    }
}

fn main() {
    let mut precision = 4usize;
    let mut words: Vec<String> = Vec::new();
    let mut args = env::args().skip(1);

    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            "-V" | "--version" => {
                println!("conv {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "-p" | "--precision" => match args.next().and_then(|v| v.parse().ok()) {
                Some(p) => precision = p,
                None => {
                    eprintln!("error: --precision needs a number");
                    process::exit(2);
                }
            },
            _ => words.push(a),
        }
    }

    if words.is_empty() {
        repl(precision);
        return;
    }

    match run(&words.join(" "), precision) {
        Ok(out) => println!("{out}"),
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(s: &str) -> String {
        run(s, 4).unwrap()
    }

    fn err(s: &str) -> String {
        run(s, 4).unwrap_err()
    }

    #[test]
    fn basics() {
        assert_eq!(c("10 km in mi"), "10 km = 6.2137 mi");
        assert_eq!(c("60 mph to m/s"), "60 mph = 26.8224 m/s");
        assert_eq!(c("1 GiB to MiB"), "1 GiB = 1024 MiB");
    }

    #[test]
    fn temperature() {
        assert_eq!(c("72 f to c"), "72 °F = 22.2222 °C");
        assert_eq!(c("-40 f to c"), "-40 °F = -40 °C");
        assert_eq!(c("0 c to k"), "0 °C = 273.15 K");
    }

    #[test]
    fn compound_input() {
        assert_eq!(c("5 ft 11 in to cm"), "5 ft 11 in = 180.34 cm");
    }

    #[test]
    fn in_is_both_unit_and_separator() {
        assert_eq!(c("10 in in cm"), "10 in = 25.4 cm");
    }

    #[test]
    fn multi_word_units_and_targets() {
        assert_eq!(c("1 fl oz to ml"), "1 fl oz = 29.5735 ml");
        assert!(c("1 km to m, ft").starts_with("1 km = 1000 m, "));
    }

    #[test]
    fn errors() {
        assert!(err("10 kg to km").contains("mass to length"));
        assert!(err("5 furlongz to m").contains("furlong"));
        assert!(err("km to mi").contains("expected a number"));
        assert!(err("10 km in").contains("expected a unit"));
        assert!(err("5 f + 5 f").contains("unexpected character"));
    }
}