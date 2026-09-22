//! Arithmetic the critic is handed rather than asked to do.
//!
//! The small model cannot multiply: with reasoning off it reads `100M * 365 * 500B = 18GB`
//! and agrees, and it cannot reliably subtract a class rate from 100. It *can* copy text.
//! So a fast call copies the candidate's calculations out of their notes, verbatim, and
//! this module does the sums — and writes the verdicts, which reach the candidate as the
//! opening lines of the critique, ahead of anything the model says. The model is told they
//! are already shown and is not allowed an arithmetic verdict of its own: measured, it
//! invents mismatches in numbers the code could not check.
//!
//! Nothing here trusts the extraction. An expression that will not parse, one quoting a
//! number the notes never contain, or one whose bare `B` cannot be read one way for sure,
//! is dropped rather than checked, because a wrong "you made a mistake" is worse than no
//! check at all.

use serde_json::Value;

/// Within this of the exact figure, a claim is simply correct.
///
/// Wider than a strict 10% on purpose: someone who counts in binary units writes 1 TB for
/// 1.0995e12 bytes, which is 9.95% off in SI terms.
const TOLERANCE: f64 = 0.15;

/// Between the tolerance and this factor either way, a claim is "roughly right". Estimates
/// are rounded on purpose — `~3K/s` for 2.5K is how people write — and the slips worth
/// interrupting for are orders of magnitude.
const ROUGH: f64 = 2.0;

/// Room for more than any real notes pane holds, and a ceiling on a model that loops.
const MAX_CALCS: usize = 8;

/// Longer than any real sum. The parser recurses, the expression is model output, and the
/// model copies from notes anyone can type into — ten thousand `(` would overflow the stack
/// and abort the whole process, not just this request. Depth is capped separately because
/// 200 characters of `-` is still 200 frames.
const MAX_EXPR: usize = 200;
const MAX_DEPTH: u32 = 32;

/// Every bare `B` is read both ways independently, so the readings tried are 2^n.
const MAX_BARE_B: u32 = 4;

/// Evaluate `+ - * /` and parentheses over decimal numbers, exactly as far as `f64` goes.
///
/// A number may carry the suffixes people write in back-of-the-envelope notes — K, M, B,
/// T, million, KB..PB, KiB..PiB, bytes, % — because the extraction copies numbers as
/// written: asked to expand them itself, the model turned `100M` into `1000000000` and
/// `500B` into `500e3`.
///
/// A bare `B` is ambiguous — `500B` per record is bytes, `2B` users is billions — so here
/// every one takes `bare_b`; `check` tries each one both ways. Returns `None` for anything
/// else, including division by zero; a check that cannot be computed is not made.
pub fn eval(expr: &str, bare_b: f64) -> Option<f64> {
    read(expr, Some(bare_b), 0).map(|r| r.value)
}

struct Reading {
    value: f64,
    /// The numbers as written, so the caller can check each appears in the notes.
    literals: Vec<String>,
    bare_bs: u32,
}

/// Parse with every bare `B` as `uniform`, or when that is `None`, the n-th bare `B` as a
/// billion when bit n of `mask` is set and as bytes when it is not.
fn read(expr: &str, uniform: Option<f64>, mask: u32) -> Option<Reading> {
    if expr.len() > MAX_EXPR {
        return None;
    }
    let mut p = Parser { s: expr.chars().collect(), pos: 0, uniform, mask, bare_bs: 0, depth: 0, literals: vec![] };
    let v = p.expr()?;
    p.skip_ws();
    (p.pos == p.s.len() && v.is_finite()).then(|| Reading { value: v, literals: p.literals, bare_bs: p.bare_bs })
}

struct Parser {
    s: Vec<char>,
    pos: usize,
    uniform: Option<f64>,
    mask: u32,
    bare_bs: u32,
    depth: u32,
    literals: Vec<String>,
}

impl Parser {
    fn skip_ws(&mut self) {
        while self.s.get(self.pos).is_some_and(|c| c.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_ws();
        self.s.get(self.pos).copied()
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.pos += 1;
            let r = self.term()?;
            v = if op == '+' { v + r } else { v - r };
        }
        Some(v)
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.factor()?;
        // `x` and `×` because people write them, and the extraction copies what they wrote.
        while let Some(op @ ('*' | '/' | 'x' | '×')) = self.peek() {
            self.pos += 1;
            let r = self.factor()?;
            v = if op == '/' {
                if r == 0.0 {
                    return None;
                }
                v / r
            } else {
                v * r
            };
        }
        Some(v)
    }

    /// The only place the parser recurses, so the only place depth is counted.
    fn factor(&mut self) -> Option<f64> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return None;
        }
        let v = match self.peek()? {
            '-' => {
                self.pos += 1;
                -self.factor()?
            }
            '(' => {
                self.pos += 1;
                let v = self.expr()?;
                (self.peek()? == ')').then(|| self.pos += 1)?;
                v
            }
            // A leading `~` is "about", which is how estimates are written.
            '~' => {
                self.pos += 1;
                self.number()?
            }
            _ => self.number()?,
        };
        self.depth -= 1;
        Some(v)
    }

    fn number(&mut self) -> Option<f64> {
        let start = self.pos;
        let digits = |p: &mut Parser| {
            // Commas are thousands separators here, never decimal points.
            while p.s.get(p.pos).is_some_and(|c| c.is_ascii_digit() || *c == ',') {
                p.pos += 1;
            }
        };
        digits(self);
        if self.s.get(self.pos) == Some(&'.') {
            self.pos += 1;
            digits(self);
        }
        // An exponent only if digits follow, so `5e` is not half a number.
        if self.s.get(self.pos) == Some(&'e') {
            let mark = self.pos;
            self.pos += 1;
            if matches!(self.s.get(self.pos), Some('+' | '-')) {
                self.pos += 1;
            }
            if self.s.get(self.pos).is_some_and(|c| c.is_ascii_digit()) {
                digits(self);
            } else {
                self.pos = mark;
            }
        }
        let raw: String = self.s[start..self.pos].iter().collect();
        // Commas only as thousands separators: `1,5` is a European one and a half, not 15.
        let whole = raw.split(['.', 'e']).next().unwrap_or_default();
        if !whole.split(',').skip(1).all(|group| group.len() == 3) {
            return None;
        }
        let mut v: f64 = raw.replace(',', "").parse().ok()?;

        // The suffix, with or without a space before it.
        let mark = self.pos;
        self.skip_ws();
        let word_start = self.pos;
        while self.s.get(self.pos).is_some_and(|c| c.is_ascii_alphabetic() || *c == '%') {
            self.pos += 1;
        }
        let word: String = self.s[word_start..self.pos].iter().collect();
        match word.as_str() {
            // `x` is the multiplication sign, not a suffix; leave it for `term`.
            "" | "x" => self.pos = mark,
            "B" => {
                let n = self.bare_bs;
                self.bare_bs += 1;
                v *= self.uniform.unwrap_or(if n < 32 && (self.mask >> n) & 1 == 1 { 1e9 } else { 1.0 });
            }
            w => v *= suffix(w, 1.0)?,
        }
        self.literals.push(self.s[start..self.pos].iter().collect());
        Some(v)
    }
}

/// What a suffix multiplies by. `bare_b` is what a lone `B` means to the caller.
fn suffix(word: &str, bare_b: f64) -> Option<f64> {
    Some(match word {
        "k" | "K" => 1e3,
        "M" => 1e6,
        "B" => bare_b,
        "T" => 1e12,
        "%" => 0.01,
        word => {
            let w = word.to_ascii_lowercase();
            match w.strip_suffix('s').unwrap_or(&w) {
                "thousand" => 1e3,
                "million" | "mn" => 1e6,
                "billion" | "bn" => 1e9,
                "trillion" => 1e12,
                _ => byte_scale(word)?,
            }
        }
    })
}

/// A byte unit as written. Case is checked before anything is lowercased: `Gb` is gigabits,
/// and read as gigabytes it made `10Gb/s * 3600 = 4.5TB` a false eight-fold MISMATCH.
fn byte_scale(word: &str) -> Option<f64> {
    if bits(word) {
        return None;
    }
    let w = word.to_ascii_lowercase();
    byte_unit(w.strip_suffix('s').unwrap_or(&w))
}

/// Bits, which this does not convert: `b`, `Kb`..`Pb` with a lowercase `b`, `bit(s)`, `*bps`.
/// Only the short forms count, so `jobs` is still a noun.
fn bits(word: &str) -> bool {
    let t = word.strip_suffix('s').unwrap_or(word);
    let w = word.to_ascii_lowercase();
    (t.len() <= 2 && t.ends_with('b')) || w.ends_with("bps") || matches!(w.as_str(), "bit" | "bits")
}

/// A unit of time, as one canonical name. Time is a dimension this does not scale, so a
/// number carrying one takes part in a sum only when a `/day`, `per day` cancels it.
fn time_unit(word: &str) -> Option<&'static str> {
    let w = word.to_ascii_lowercase();
    let stem = if w.len() > 2 { w.strip_suffix('s').unwrap_or(&w) } else { &w };
    Some(match stem {
        "ns" => "ns",
        "us" | "µs" => "us",
        "ms" => "ms",
        "s" | "sec" | "second" => "s",
        "min" | "minute" => "min",
        "h" | "hr" | "hour" => "h",
        "day" => "day",
        "week" => "week",
        "month" => "month",
        "year" | "yr" => "year",
        _ => return None,
    })
}

/// A byte unit, lowercased with any plural `s` already gone.
fn byte_unit(w: &str) -> Option<f64> {
    Some(match w {
        "byte" => 1.0,
        "kb" | "kilobyte" => 1e3,
        "mb" | "megabyte" => 1e6,
        "gb" | "gigabyte" => 1e9,
        "tb" | "terabyte" => 1e12,
        "pb" | "petabyte" => 1e15,
        "kib" => 1024f64,
        "mib" => 1024f64.powi(2),
        "gib" => 1024f64.powi(3),
        "tib" => 1024f64.powi(4),
        "pib" => 1024f64.powi(5),
        _ => return None,
    })
}

/// Whether any word in `s` is a byte unit — `18GB/day`, `500 bytes`. Word by word, so
/// `numbers` is not read as containing `mb`.
fn has_byte_unit(s: &str) -> bool {
    s.split(|c: char| !c.is_ascii_alphabetic()).any(|w| byte_scale(w).is_some())
}

/// A claimed result as written — `18GB`, `~1157 writes/sec`, `36.5 billion` — read as its
/// leading quantity.
///
/// A word after a space that is not a unit is a noun (`writes`, `users`) and the bare number
/// stands. Anything else unknown — letters glued to the number, or a word that looks like a
/// unit this does not know — is `None`: reading `36.5 billion` as 36.5 is a false billion-fold
/// MISMATCH, and no check beats that.
fn quantity(claimed: &str, bare_b: f64) -> Option<f64> {
    let t = claimed.trim().trim_start_matches(['~', '≈']).trim_start();
    let end = t.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).unwrap_or(t.len());
    let (number, rest) = t.split_at(end);
    let n = eval(number, bare_b)?;
    let glued = !rest.starts_with(char::is_whitespace);
    let rest = rest.trim_start();
    let word = &rest[..rest.find(|c: char| !(c.is_ascii_alphabetic() || c == '%')).unwrap_or(rest.len())];
    if word.is_empty() {
        return Some(n);
    }
    match suffix(word, bare_b) {
        Some(m) => Some(n * m),
        None if glued || unit_like(word) => None,
        None => Some(n),
    }
}

/// A unit this cannot scale, where reading the bare number would compare the wrong thing:
/// `168 hours/week`, `5 s`, `40 Gbps`.
fn unit_like(word: &str) -> bool {
    let w = word.to_ascii_lowercase();
    bits(word) || time_unit(word).is_some() || ["byte", "illion", "flop"].iter().any(|u| w.contains(u))
}

/// How a claimed figure compares with the exact one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Correct,
    Rough,
    Off,
}

/// One of the candidate's calculations, redone.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub expression: String,
    /// As the notes wrote it. Kept for tests and never shown: it is free text the model
    /// copied, and whatever rides along in it would ride into the critique too.
    pub claimed: String,
    pub exact: f64,
    pub claimed_value: f64,
    pub bytes: bool,
}

impl Check {
    pub fn verdict(&self) -> Verdict {
        let r = self.claimed_value / self.exact;
        if (r - 1.0).abs() <= TOLERANCE {
            Verdict::Correct
        } else if (1.0 / ROUGH..=ROUGH).contains(&r) {
            Verdict::Rough
        } else {
            Verdict::Off
        }
    }

    pub fn mismatch(&self) -> bool {
        self.verdict() == Verdict::Off
    }

    /// The line the candidate reads. Built only from the parsed expression and two computed
    /// numbers, so nothing the model wrote reaches the page unparsed.
    fn report(&self) -> String {
        let expr = self.expression.split_whitespace().collect::<Vec<_>>().join(" ").replace('*', "×");
        let r = self.claimed_value / self.exact;
        let verdict = match self.verdict() {
            Verdict::Correct => "correct".to_string(),
            Verdict::Rough => {
                format!("roughly right, {}% {}", sig((r - 1.0).abs() * 100.0, 2), if r > 1.0 { "high" } else { "low" })
            }
            Verdict::Off => {
                let (f, dir) = if r > 1.0 { (r, "large") } else { (1.0 / r, "small") };
                format!("about {}× too {dir}", sig(f, 2))
            }
        };
        format!(
            "Arithmetic check: {expr} = {}. You wrote {}: {verdict}.",
            human(self.exact, self.bytes),
            human(self.claimed_value, self.bytes),
        )
    }
}

/// The accuracy a model scores by always predicting the majority class, next to theirs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Baseline {
    /// Percent of cases that are positive, as a percentage (0.5 means half a percent).
    pub positive_rate: f64,
    pub accuracy: f64,
}

impl Baseline {
    /// The finished sentence. Computed here because the model, asked to subtract 0.5 from
    /// 100, turns 0.5% into 0.005 first and answers 99.995.
    pub fn sentence(&self) -> String {
        let (r, a) = (self.positive_rate, self.accuracy);
        // Always-no is the right trivial model only while positives are the minority.
        let (label, base) = if r <= 50.0 {
            (format!("Always-no baseline: 100 - {} = {}%.", dec(r), dec(100.0 - r)), 100.0 - r)
        } else {
            (format!("Always-yes baseline: {}%.", dec(r)), r)
        };
        let verdict = if a < base {
            format!(
                "That is {} points below the baseline: the model is worse than one that never \
                 predicts the minority class at all.",
                dec(base - a)
            )
        } else if a == base {
            "That is exactly the baseline: the model does no better than one that always gives \
             the same answer."
                .to_string()
        } else {
            format!("That is {} points above the baseline.", dec(a - base))
        };
        format!("{label} Your accuracy: {}%. {verdict}", dec(a))
    }
}

/// Everything recomputed for one critique. Empty is the normal case, and means the prompt
/// goes out exactly as it did before this module existed.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Checks {
    pub calcs: Vec<Check>,
    pub baseline: Option<Baseline>,
}

/// Which parts of an extraction a stage uses. The estimation stage grades the sums; the
/// ML modelling and evaluation stages grade the accuracy figure. Handing either the other
/// one is a distraction the ignore-half of `Stage::focus` exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wanted {
    Calculations,
    Baseline,
}

impl Checks {
    pub fn is_empty(&self) -> bool {
        self.calcs.is_empty() && self.baseline.is_none()
    }

    /// Read the extraction's answer. Defensive throughout: garbage in means no checks,
    /// never an error, and a check that cannot be tied back to the notes is dropped.
    pub fn from_extraction(raw: &str, notes: &str, wanted: Wanted) -> Checks {
        // A model may wrap the object in a code fence or a sentence; take the outermost braces.
        let json = match (raw.find('{'), raw.rfind('}')) {
            (Some(a), Some(b)) if a < b => &raw[a..=b],
            _ => return Checks::default(),
        };
        let Ok(v) = serde_json::from_str::<Value>(json) else {
            return Checks::default();
        };
        let segments = segments(notes);

        match wanted {
            Wanted::Calculations => {
                let calcs = v["calculations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| {
                        check(
                            c["expression"].as_str()?,
                            c["claimed"].as_str()?,
                            c["unit"].as_str().unwrap_or_default(),
                            &segments,
                        )
                    })
                    .take(MAX_CALCS)
                    .collect();
                Checks { calcs, baseline: None }
            }
            Wanted::Baseline => {
                // Both must be percentages copied from the notes. A bare number is where the
                // model's 0.5-becomes-0.005 slip would sneak back in. And the accuracy must
                // come from a sentence that says "accuracy", the rate from one that does not:
                // handed recall and precision, the extraction calls one of them accuracy, and
                // a baseline compared against recall is a false verdict.
                let sents: Vec<(String, Segment)> = sentences(notes).map(|s| (s.to_lowercase(), segment(s))).collect();
                let pct = |key: &str, accuracy: bool| {
                    let s = v[key].as_str()?.trim();
                    let found = sents.iter().any(|(text, seg)| occurs(seg, s) && text.contains("accura") == accuracy);
                    if !s.ends_with('%') || !found {
                        return None;
                    }
                    s.trim_end_matches('%').trim().parse::<f64>().ok().filter(|p| (0.0..=100.0).contains(p))
                };
                let baseline = pct("positive_rate", false)
                    .zip(pct("accuracy", true))
                    .map(|(positive_rate, accuracy)| Baseline { positive_rate, accuracy });
                Checks { calcs: vec![], baseline }
            }
        }
    }

    /// Sent to the candidate ahead of the model's answer, as its opening lines. Empty when
    /// there is nothing to say.
    pub fn report(&self) -> String {
        let mut lines: Vec<String> = self.calcs.iter().map(Check::report).collect();
        lines.extend(self.baseline.map(|b| b.sentence()));
        if lines.is_empty() {
            return String::new();
        }
        lines.join("\n") + "\n\n"
    }

    /// Appended to the system prompt. The model is not allowed an arithmetic verdict of its
    /// own: given checked figures and told to trust them, it still did its own sums on the
    /// lines code could not check, and called a correct 400M wrong three times out of three.
    ///
    /// It does not quote the report. Quoted under a header, the model copied header and all
    /// into its answer in 2 of 6 runs; told that "code" had checked, it told the candidate so.
    /// It needs to know only that the arithmetic is dealt with, not what was said.
    pub fn instruction(&self) -> String {
        let mut out = String::new();
        if !self.calcs.is_empty() {
            out.push_str(
                "\n\nThe candidate has already been shown a check of the arithmetic in their notes, \
directly above your answer. Do not mention it or repeat any of its figures, and do not say \
whether any number in their notes is right or wrong. Grade their assumptions and what is missing.",
            );
        }
        if let Some(b) = self.baseline {
            let which = if b.positive_rate > 50.0 { "always-yes" } else { "always-no" };
            out.push_str(&format!(
                "\n\nYour answer follows a sentence the candidate has already read, comparing their \
accuracy with the {which} baseline and saying which is higher. That is settled: do not \
mention their accuracy, the baseline, or what any trivial model would score. Go straight on \
to grading the rest.",
            ));
        }
        out
    }
}

/// Sums written out as `<numbers and operators> = <result>`, found with no model at all.
///
/// Measured: on `100M new URLs/day * 365 * 500B = 18GB/year` the extraction answered
/// `1000000000 * 365 * 500` five times out of five, which the notes check rightly drops —
/// so the one case this module exists for went unchecked. Written-out sums are the common
/// case and need no model; the extraction is only asked when this finds nothing.
pub fn scan(notes: &str) -> Checks {
    let segs = segments(notes);
    let mut calcs = vec![];
    for sentence in sentences(notes) {
        let sides: Vec<&str> = sentence.split('=').collect();
        for pair in sides.windows(2) {
            // `Storage: 100M * ...` — the label is not part of the sum.
            let left = pair[0].rsplit(':').next().unwrap_or(pair[0]);
            if let Some(c) = operands(left).and_then(|e| check(&e, pair[1], "", &segs)) {
                calcs.push(c);
            }
        }
    }
    calcs.truncate(MAX_CALCS);
    Checks { calcs, baseline: None }
}

/// The last run of `number op number ...` in `text`, with the words dropped:
/// `100M new URLs/day * 365 days * 500B` is `100M * 365 * 500B`.
///
/// An operator counts only between two numbers — the `/` in `URLs/day` is "per", and is
/// followed by a word. Two numbers with no operator between start the run again, so
/// `2B users, 10% active` is not a sum at all. `None` for a single number, and for anything
/// with brackets: a missed bracket reorders the sum, and the extraction handles those.
// ponytail: no brackets; the model extraction is the fallback for `(a + b) * c`.
fn operands(text: &str) -> Option<String> {
    if text.contains(['(', ')']) || unscalable(text) {
        return None;
    }
    let s: Vec<char> = text.chars().collect();
    let starts_number = |i: usize| {
        let j = (i..s.len()).find(|&j| !s[j].is_whitespace()).unwrap_or(s.len());
        s.get(j).is_some_and(|c| c.is_ascii_digit() || *c == '~')
    };
    let (mut out, mut ops, mut pending) = (Vec::<String>::new(), 0, None::<char>);
    let mut last_was_number = false;
    let mut last_end = 0;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c.is_ascii_digit() || (c == '~' && s.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            i += 1;
            while s.get(i).is_some_and(|c| c.is_ascii_digit() || matches!(c, ',' | '.')) {
                i += 1;
            }
            // An exponent, so `1e6` is one number and not `1`, `e` and `6`.
            if s.get(i) == Some(&'e') && s.get(i + 1).is_some_and(char::is_ascii_digit) {
                i += 1;
                while s.get(i).is_some_and(char::is_ascii_digit) {
                    i += 1;
                }
            }
            // A trailing `,` or `.` is punctuation, not part of the number.
            while matches!(s[i - 1], ',' | '.') {
                i -= 1;
            }
            let mut tok: String = s[start..i].iter().collect();
            // A suffix glued on, or after a space if it is one (`500 bytes`, not `365 days`).
            let j = (i..s.len()).find(|&j| !s[j].is_whitespace()).unwrap_or(s.len());
            let k = (j..s.len()).find(|&k| !(s[k].is_ascii_alphabetic() || s[k] == '%')).unwrap_or(s.len());
            let word: String = s[j..k].iter().collect();
            if !word.is_empty() && word != "x" && suffix(&word, 1.0).is_some() {
                tok.push_str(&s[i..k].iter().collect::<String>());
                i = k;
            }
            match pending.take() {
                Some(op) if last_was_number => {
                    out.push(op.to_string());
                    ops += 1;
                }
                _ => {
                    out.clear();
                    ops = 0;
                }
            }
            out.push(tok);
            last_was_number = true;
            last_end = i;
        } else if c.is_ascii_alphabetic() {
            // A name with digits in it — `H100`, `EC2`, `m5.large` — is one word, not a
            // word and a number: `1000 H100 GPUs` must not multiply by 100.
            let name = |k: usize| {
                s[k].is_ascii_alphanumeric() || (s[k] == '.' && s.get(k + 1).is_some_and(char::is_ascii_alphanumeric))
            };
            let k = (i..s.len()).find(|&k| !name(k)).unwrap_or(s.len());
            let word: String = s[i..k].iter().collect();
            if word == "x" && last_was_number && starts_number(k) {
                pending = Some('*');
            }
            i = k;
        } else {
            if matches!(c, '*' | '/' | '+' | '-' | '×') && last_was_number && starts_number(i + 1) {
                // `5 users - 3` is more likely a range or an aside than a subtraction.
                if c == '-' && !s[last_end..i].iter().all(|c| c.is_whitespace()) {
                    return None;
                }
                pending = Some(if c == '×' { '*' } else { c });
            }
            i += 1;
        }
    }
    (ops > 0).then(|| out.join(" "))
}

/// Whether a sentence carries a unit this cannot scale, so no sum in it can be checked.
///
/// Bits anywhere, and any unit of time that is not divided out. `10 Mbps * 3600 = 4.5 GB`,
/// `0.5 ms * 10000 = 5 s` and `Uptime 24/7 = 168 hours/week` are all right, and each was
/// called wrong with its unit read as a noun. Time that cancels is fine: in `100M URLs/day *
/// 365 days` the `/day` takes the days back out, and so do `per day` and `a day`.
fn unscalable(text: &str) -> bool {
    let (mut carried, mut per) = (Vec::new(), Vec::new());
    let mut before = ' ';
    let mut prev = String::new();
    let mut word = String::new();
    for c in text.chars().chain([' ']) {
        if c.is_ascii_alphabetic() {
            word.push(c);
            continue;
        }
        if !word.is_empty() {
            if bits(&word) {
                return true;
            }
            if let Some(unit) = time_unit(&word) {
                let divides = before == '/' || ["per", "a", "an", "each", "every"].iter().any(|p| prev.eq_ignore_ascii_case(p));
                if divides { per.push(unit) } else { carried.push(unit) }
            }
            prev = std::mem::take(&mut word);
        }
        // `before` is the last non-space character, so a word right after `/` sees the slash;
        // `prev` survives spaces, so `day` in `per day` sees `per`, and nothing else.
        if !c.is_whitespace() {
            before = c;
            prev.clear();
        }
    }
    carried.iter().any(|u| !per.contains(u))
}

/// The notes as lines and sentences. A calculation's numbers and its result must all sit in
/// one of these: a literal found in one sentence and a result in another is two
/// coincidences, not a sum.
fn sentences(notes: &str) -> impl Iterator<Item = &str> {
    notes.split('\n').flat_map(|l| l.split(". "))
}

/// A sentence with its whitespace removed, each character remembering whether whitespace
/// came before it — so `500 bytes` matches `500bytes`, yet `, 500` is not read as the
/// thousands separator `,500`.
type Segment = Vec<(char, bool)>;

fn segment(s: &str) -> Segment {
    let mut out = vec![];
    let mut space = true;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            out.push((c, space));
            space = false;
        }
    }
    out
}

fn segments(notes: &str) -> Vec<Segment> {
    sentences(notes).map(segment).collect()
}

/// Whether `needle` appears in `hay` as a whole number rather than the tail or head of a
/// bigger one: `2%` is not in `0.2%`, and `500B` is not in `1500B`.
fn occurs(hay: &[(char, bool)], needle: &str) -> bool {
    last(hay, needle).is_some()
}

/// Where the last whole-number occurrence of `needle` in `hay` starts.
fn last(hay: &[(char, bool)], needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().filter(|c| !c.is_whitespace()).collect();
    let n = needle.len();
    if n == 0 || n > hay.len() {
        return None;
    }
    (0..=hay.len() - n).rev().find(|&i| {
        let joined = |k: usize| hay.get(k).is_some_and(|(_, space)| !space);
        hay[i..i + n].iter().map(|(c, _)| *c).eq(needle.iter().copied())
            // Not the tail of a number, or of a name: `100` is not in `H100`.
            && (hay[i].1 || !(hay[i - 1].0.is_ascii_alphanumeric() || matches!(hay[i - 1].0, '.' | ',')))
            // Nor the head of one: `1` is not in `1.5`.
            && !(joined(i + n) && hay[i + n].0.is_ascii_digit())
            && !(joined(i + n) && hay[i + n].0 == '.' && joined(i + n + 1) && hay[i + n + 1].0.is_ascii_digit())
    })
}

/// Redo one extracted calculation, or decline to.
fn check(expression: &str, claimed: &str, unit: &str, segments: &[Segment]) -> Option<Check> {
    let claimed = claimed.trim();
    let bytes = unit.eq_ignore_ascii_case("bytes") || has_byte_unit(claimed);
    let claimed_value = quantity(claimed, if bytes { 1.0 } else { 1e9 }).filter(|v| *v > 0.0)?;
    let first = read(expression, None, 0)?;

    // The result and every number must come from one sentence of the notes, bar the two
    // constants the extraction is told it may add, and every number must come *before* the
    // result. Measured: asked about prose that never wrote a sum down, MiniCPM5-2B invented
    // `50 * 365 * 24 * 3600` for "50GB/day, so about 18TB a year"; handed `((((1 = 5GB`, it
    // answered `1 * 5` claiming `5GB` — the 5 found in the notes only as the claim itself —
    // and that went out as a billion-fold MISMATCH.
    //
    // The extraction may add two constants the notes never write — "a year" is 365 and "per
    // second" from a daily figure is 86400 — but only with their reason in the same sentence.
    // Any other factor missing from the notes was invented, and an invented factor is how an
    // honest calculation gets called wrong.
    let found_in = |seg: &Segment| {
        let spaced: String = seg.iter().flat_map(|&(c, space)| [(space).then_some(' '), Some(c)]).flatten().collect();
        if unscalable(&spaced) {
            return false;
        }
        let text: String = seg.iter().map(|(c, _)| c.to_ascii_lowercase()).collect();
        let implied = |l: &str| match l {
            "365" => ["year", "annual", "yr"].iter().any(|w| text.contains(w)),
            "86400" => ["sec", "/s"].iter().any(|w| text.contains(w)),
            _ => false,
        };
        last(seg, claimed).is_some_and(|p| first.literals.iter().all(|l| occurs(&seg[..p], l) || implied(l)))
    };
    if !segments.iter().any(found_in) {
        return None;
    }

    // Each bare `B` both ways, independently: `36.5B URLs * 500B` is a billion and a byte.
    if first.bare_bs > MAX_BARE_B {
        return None;
    }
    let readings: Vec<f64> =
        (0..1u32 << first.bare_bs).map(|m| read(expression, None, m).map_or(f64::NAN, |r| r.value)).collect();
    let (best, &exact) = readings
        .iter()
        .enumerate()
        .filter(|(_, v)| **v > 0.0)
        .min_by(|(_, x), (_, y)| distance(**x, claimed_value).total_cmp(&distance(**y, claimed_value)))?;

    let found = Check { expression: expression.trim().to_string(), claimed: claimed.to_string(), exact, claimed_value, bytes };
    // When no reading fits, a bare `B` is a guess at which slip they made — unless it is the
    // only thing in the sum that can be bytes, the result is bytes, and bytes is also the
    // nearer reading. That is the 18GB-that-is-18TB case exactly, and nothing wider.
    if found.mismatch() && first.bare_bs > 0 {
        let only_bytes = bytes && first.bare_bs == 1 && best == 0 && !first.literals.iter().any(|l| has_byte_unit(l));
        if !only_bytes {
            return None;
        }
    }
    Some(found)
}

/// Orders of magnitude between two figures; what "nearer" means for readings that can
/// differ by a factor of a billion.
fn distance(a: f64, b: f64) -> f64 {
    if a <= 0.0 || b <= 0.0 {
        return f64::INFINITY;
    }
    (a / b).log10().abs()
}

/// A figure in the units a reader thinks in: `18.25 TB`, `36.5 billion`, `1157.41`.
fn human(v: f64, bytes: bool) -> String {
    if bytes {
        let units = ["bytes", "KB", "MB", "GB", "TB", "PB", "EB"];
        let mut v = v;
        let mut i = 0;
        while v.abs() >= 1000.0 && i < units.len() - 1 {
            v /= 1000.0;
            i += 1;
        }
        return format!("{} {}", dec(v), units[i]);
    }
    // Words, not letters, for counts: `B` is exactly the ambiguity this module exists for.
    for (scale, word) in [(1e12, "trillion"), (1e9, "billion"), (1e6, "million")] {
        if v.abs() >= scale {
            return format!("{} {word}", dec(v / scale));
        }
    }
    dec(v)
}

/// Two decimals, trailing zeros dropped — `99.5`, `18.25`, `4` — or two significant figures
/// below 0.01, so a small rate is `0.005` rather than `0`.
fn dec(v: f64) -> String {
    let places = if v != 0.0 && v.abs() < 0.01 { (-v.abs().log10().floor()) as usize + 1 } else { 2 };
    let s = format!("{:.*}", places, v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// `n` significant figures, for the size of a slip: `1000`, not `1013.89`.
fn sig(v: f64, n: i32) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let mag = 10f64.powi(v.abs().log10().floor() as i32 - n + 1);
    dec((v / mag).round() * mag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str) -> Option<f64> {
        eval(s, 1.0)
    }

    fn calcs(raw: &str, notes: &str) -> Vec<Check> {
        Checks::from_extraction(raw, notes, Wanted::Calculations).calcs
    }

    fn one(expression: &str, claimed: &str, unit: &str, notes: &str) -> Vec<Check> {
        let raw = serde_json::json!({ "calculations": [{ "expression": expression, "claimed": claimed, "unit": unit }] });
        calcs(&raw.to_string(), notes)
    }

    #[test]
    fn precedence_and_parentheses() {
        assert_eq!(ev("2 + 3 * 4"), Some(14.0));
        assert_eq!(ev("(2 + 3) * 4"), Some(20.0));
        assert_eq!(ev("10 - 4 - 3"), Some(3.0), "subtraction is left-associative");
        assert_eq!(ev("100 / 10 / 2"), Some(5.0), "so is division");
        assert_eq!(ev("-(3 - 5) * 2"), Some(4.0));
        assert_eq!(ev("((1))"), Some(1.0));
        assert_eq!(ev("100 - 0.5"), Some(99.5));
    }

    #[test]
    fn numbers_as_people_write_them() {
        assert_eq!(ev("1.5e3"), Some(1500.0));
        assert_eq!(ev("2.5E-1"), None, "only a lowercase exponent");
        assert_eq!(ev("1e-3 * 1000"), Some(1.0));
        assert_eq!(ev("1,000,000"), Some(1e6));
        assert_eq!(ev("100M * 365"), Some(36.5e9));
        assert_eq!(ev("20K x 3"), Some(60e3));
        assert_eq!(ev("20k × 3"), Some(60e3));
        assert_eq!(ev("4 KB * 1000"), Some(4e6));
        assert_eq!(ev("500 bytes"), Some(500.0));
        assert_eq!(ev("36.5 billion"), Some(36.5e9));
        assert_eq!(ev("2 thousand * 3 mn"), Some(6e9));
        assert_eq!(ev("1 KiB"), Some(1024.0));
        assert_eq!(ev("1 TiB"), Some(1024f64.powi(4)));
        assert_eq!(ev("2 gigabytes"), Some(2e9));
        assert_eq!(ev("2B * 10%"), Some(0.2), "bare B follows the caller's reading");
        assert_eq!(eval("2B * 10%", 1e9), Some(2e8));
        assert_eq!(ev("~1157"), Some(1157.0));
    }

    #[test]
    fn the_18gb_that_is_18tb() {
        // The case from CLAUDE.md, verbatim as the extraction copies it.
        let v = ev("100M * 365 * 500B").unwrap();
        assert_eq!(v, 18.25e12);
        assert_eq!(human(v, true), "18.25 TB");
    }

    #[test]
    fn refuses_what_it_cannot_compute() {
        assert_eq!(ev("1 / 0"), None, "division by zero is no check, not infinity");
        assert_eq!(ev("1 / (2 - 2)"), None);
        assert_eq!(ev(""), None);
        assert_eq!(ev("2 +"), None);
        assert_eq!(ev("(2 + 3"), None);
        assert_eq!(ev("2 + 3)"), None, "trailing input is a rejection, not ignored");
        assert_eq!(ev("2 ^ 3"), None);
        assert_eq!(ev("sqrt(4)"), None);
        assert_eq!(ev("5 apples"), None, "an unknown suffix is not silently a 1");
        assert_eq!(ev("100:1"), None);
        assert_eq!(ev("1e999"), None, "overflow is not a result");
    }

    #[test]
    fn hostile_nesting_is_refused_not_a_stack_overflow() {
        for bomb in ["-".repeat(10_000) + "1", "(".repeat(10_000) + "1" + &")".repeat(10_000)] {
            assert_eq!(ev(&bomb), None);
        }
        // Under the length cap, so this is the depth counter's refusal.
        assert_eq!(ev(&("-".repeat(100) + "1")), None);
        assert_eq!(ev(&("(".repeat(60) + "1" + &")".repeat(60))), None);
        // And ordinary nesting still works.
        assert_eq!(ev(&("(".repeat(10) + "1" + &")".repeat(10))), Some(1.0));
        assert_eq!(ev("--1"), Some(1.0));
    }

    #[test]
    fn claimed_results_read_their_leading_quantity() {
        assert_eq!(quantity("18GB", 1.0), Some(18e9));
        assert_eq!(quantity("18.25TB per year", 1.0), Some(18.25e12));
        assert_eq!(quantity("~1157 writes/sec", 1e9), Some(1157.0));
        assert_eq!(quantity("230/s", 1e9), Some(230.0));
        assert_eq!(quantity("50 GB/day", 1.0), Some(50e9));
        assert_eq!(quantity("36.5B", 1e9), Some(36.5e9));
        assert_eq!(quantity("fast", 1.0), None);
    }

    #[test]
    fn a_scale_word_is_never_dropped() {
        // Read as the bare 36.5, each of these was a false billion-fold MISMATCH.
        assert_eq!(quantity("36.5 billion", 1e9), Some(36.5e9));
        assert_eq!(quantity("36.5 billion URLs", 1e9), Some(36.5e9));
        assert_eq!(quantity("2.5 mn/day", 1e9), Some(2.5e6));
        assert_eq!(quantity("1 TiB", 1.0), Some(1024f64.powi(4)));
        assert_eq!(quantity("18 gigabytes", 1.0), Some(18e9));
        // A unit this does not know is no reading, not a bare number.
        assert_eq!(quantity("36.5 zillion", 1e9), None);
        assert_eq!(quantity("5 exabytes", 1.0), None);
        assert_eq!(quantity("40Gbps", 1.0), None, "letters glued to the number");
        assert_eq!(quantity("40 Gbps", 1.0), None);
    }

    const NOTES: &str = "Write traffic: 100M new URLs/day.\nStorage: 100M new URLs/day * 365 days * 500B = 18GB per year.";

    #[test]
    fn a_unit_slip_is_a_mismatch_by_the_factor_it_is() {
        let checks = Checks {
            calcs: one("100M * 365 * 500B", "18GB", "bytes", NOTES),
            baseline: None,
        };
        assert_eq!(checks.calcs.len(), 1);
        assert!(checks.calcs[0].mismatch());
        assert_eq!(
            checks.report(),
            "Arithmetic check: 100M × 365 × 500B = 18.25 TB. You wrote 18 GB: about 1000× too small.\n\n",
        );
        assert!(!checks.instruction().contains("18"), "the model is not handed figures to echo");
        assert!(checks.instruction().contains("do not say whether any number"));
    }

    #[test]
    fn a_correct_sum_is_not_accused() {
        let notes = "100M new URLs/day / 86400 = ~1157 writes/sec. 100M * 365 * 500 bytes = 18.25TB per year. \
                     Binary: 1024 GB = 1 TB.";
        let raw = r#"{"calculations": [
            {"expression": "100M / 86400", "claimed": "~1157 writes/sec", "unit": "count"},
            {"expression": "100M * 365 * 500 bytes", "claimed": "18.25TB", "unit": "bytes"},
            {"expression": "1024 GB", "claimed": "1 TB", "unit": "bytes"}
        ]}"#;
        let checks = Checks::from_extraction(raw, notes, Wanted::Calculations);
        assert_eq!(checks.calcs.len(), 3);
        assert!(checks.calcs.iter().all(|c| c.verdict() == Verdict::Correct), "{:?}", checks.calcs);
        assert!(!checks.report().contains("too"), "{}", checks.report());
    }

    #[test]
    fn rounding_is_roughly_right_not_a_mismatch() {
        let c = one("10M / 4000", "~3K/s", "count", "10M / 4000 = ~3K/s");
        assert_eq!(c[0].verdict(), Verdict::Rough, "{c:?}");
        assert!(c[0].report().contains("roughly right, 20% high"), "{}", c[0].report());
    }

    #[test]
    fn a_calculation_the_notes_never_wrote_is_dropped() {
        // Measured: MiniCPM5-2B's extraction of prose that states results without sums.
        let notes = "Assume 100M new URLs a day, 500 bytes each. That is 50GB/day, so about 18TB a year.";
        let raw = r#"{"calculations": [
            {"expression": "50 * 365 * 24 * 3600", "claimed": "18TB", "unit": "bytes"},
            {"expression": "100000000 * 500", "claimed": "50GB", "unit": "bytes"},
            {"expression": "100M * 500", "claimed": "50GB", "unit": "bytes"},
            {"expression": "100M * 500", "claimed": "12GB", "unit": "bytes"},
            {"expression": "100M * 500", "claimed": "", "unit": "bytes"}
        ]}"#;
        // The one built from their own numbers and their own result would survive, but its
        // inputs sit in a different sentence from its result, so it does not either.
        assert!(calcs(raw, notes).is_empty());
        let one_line = notes.replace(". That", ", that");
        let c = calcs(raw, &one_line);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].claimed, "50GB");
        assert!(!c[0].mismatch());
    }

    #[test]
    fn a_number_found_only_in_the_claim_is_not_an_input() {
        // Measured: the extraction's answer to 5000 `(` then `1 = 5GB`.
        let notes = "(".repeat(5000) + "1 = 5GB";
        assert!(one("1 * 5", "5GB", "bytes", &notes).is_empty());
        assert!(one("1 * 5", "5GB", "bytes", "1 = 5GB").is_empty());
        // Inputs after the result are not a sum either.
        assert!(one("100M * 500B", "50TB", "bytes", "50TB is 100M * 500B").is_empty());
    }

    #[test]
    fn a_number_is_matched_whole_not_as_the_tail_of_another() {
        assert!(one("100M * 500B", "50TB", "bytes", "100M * 1500B = 50TB").is_empty(), "500B is not in 1500B");
        assert!(occurs(&segment("rate 0.2%"), "0.2%"));
        assert!(!occurs(&segment("rate 0.2%"), "2%"));
        assert!(!occurs(&segment("x=500"), "50"));
        assert!(occurs(&segment("a day, 500 bytes each"), "500bytes"), "a space is a boundary");
        assert!(!occurs(&segment("1,500 bytes"), "500 bytes"));
        let notes = "Spam is 0.2% of messages. Our model scores 95% accuracy.";
        let raw = r#"{"positive_rate": "2%", "accuracy": "95%"}"#;
        assert!(Checks::from_extraction(raw, notes, Wanted::Baseline).baseline.is_none());
    }

    #[test]
    fn each_bare_b_is_read_on_its_own() {
        // A billion URLs and bytes per URL, in one sum. Reading both Bs the same way is a
        // false billion-fold MISMATCH either way round.
        let c = one("36.5B * 500B", "18.25TB", "bytes", "36.5B URLs * 500B = 18.25TB");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].verdict(), Verdict::Correct, "{c:?}");

        let c = one("2B * 100", "200GB", "bytes", "2B users * 100 = 200GB");
        assert_eq!(c[0].verdict(), Verdict::Correct, "2B users is two billion here: {c:?}");
    }

    #[test]
    fn a_bare_b_that_fits_no_reading_is_dropped_not_guessed() {
        // 1000x out either way round, and which slip they made is a guess.
        assert!(one("2B * 100", "200TB", "bytes", "2B users * 100 = 200TB").is_empty());
        // Too many to try.
        assert!(one("1B*1B*1B*1B*1B", "5GB", "bytes", "1B*1B*1B*1B*1B = 5GB").is_empty());
        // Except the one case that is not a guess: the B is the only thing that can be bytes.
        assert_eq!(one("100M * 365 * 500B", "18GB", "bytes", NOTES).len(), 1);
    }

    #[test]
    fn a_written_out_sum_is_found_without_a_model() {
        let words = |notes: &str| scan(notes).calcs.iter().map(|c| (c.expression.clone(), c.verdict())).collect::<Vec<_>>();
        // The flagship, in the tester's exact wording — where the extraction answered
        // `1000000000 * 365 * 500` five times in five.
        assert_eq!(
            words("Storage: 100M new URLs/day * 365 * 500B = 18GB/year"),
            [("100M * 365 * 500B".to_string(), Verdict::Off)]
        );
        assert_eq!(scan("100M new URLs/day * 365 days * 500B = 18GB per year.").report().lines().next(),
            Some("Arithmetic check: 100M × 365 × 500B = 18.25 TB. You wrote 18 GB: about 1000× too small."));
        // A correct estimate, with a per-day slash and a rate.
        let c = words(
            "Write traffic: 100M new URLs/day / 86400 = ~1157 writes/sec.\nReads are 100x writes = ~115K reads/sec.\n\
             Storage: 100M * 365 * 500 bytes = 18.25TB per year.",
        );
        assert_eq!(c.len(), 2, "{c:?}");
        assert!(c.iter().all(|(_, v)| *v == Verdict::Correct), "{c:?}");
        // The tweet estimate a model called wrong three times in three: every sum is right,
        // and the lines that are not sums are not checked.
        let c = words(
            "2B users, 10% daily active = 200M DAU. Each posts 2 tweets/day = 400M tweets/day.\n\
             400M / 86400 = ~4630 tweets/s. Each tweet 300 bytes: 400M * 300 bytes = 120GB/day, about 44TB a year.",
        );
        assert_eq!(c.len(), 2, "{c:?}");
        assert!(c.iter().all(|(_, v)| *v == Verdict::Correct), "{c:?}");
        // A billion and a byte in one sum.
        assert_eq!(words("36.5B URLs * 500B = 18.25TB")[0].1, Verdict::Correct);
    }

    #[test]
    fn a_unit_this_cannot_scale_declines_the_whole_sum() {
        // All three are right, and each was called wrong when its unit was read as a noun.
        for notes in ["10 Mbps * 3600 = 4.5 GB", "0.5 ms * 10000 = 5 s", "Uptime 24/7 = 168 hours/week"] {
            assert!(scan(notes).is_empty(), "{notes}: {:?}", scan(notes).calcs);
        }
        // Bits written with a lowercase b, glued on: eight-fold if read as bytes.
        assert!(scan("10Gb/s * 3600 = 4.5TB").is_empty());
        assert!(scan("10 Gb * 3600 = 4.5TB").is_empty());
        // The extraction drops the words, so the same rule is applied to the sentence it
        // copied from — `10 * 3600` claiming 4.5 GB is the Mbps line with its unit gone.
        assert!(one("10 * 3600", "4.5 GB", "bytes", "10 Mbps for an hour, 10 * 3600 seconds, gives 4.5 GB").is_empty());
        assert!(one("0.5 * 10000", "5", "count", "0.5 ms each, 10000 calls, 5 in total").is_empty());
        // Time that cancels is fine: `/day` times days.
        assert_eq!(scan("100M new URLs/day * 365 days * 500B = 18GB").calcs.len(), 1);
        assert_eq!(scan("100M URLs per day * 365 days * 500B = 18.25TB").calcs.len(), 1);
    }

    #[test]
    fn a_name_with_digits_is_not_a_number() {
        assert!(scan("1000 H100 GPUs * 8 hours = 8000 GPU-hours").is_empty());
        let c = scan("8 A100 GPUs * 24 = 192 GPUs").calcs;
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!((c[0].expression.as_str(), c[0].verdict()), ("8 * 24", Verdict::Correct));
        assert!(scan("p4d * 3 = 12").is_empty());
        assert!(scan("Run on EC2 m5.large * 4 = 8 vCPUs").is_empty());
        // And a literal is never found inside a name.
        assert!(!occurs(&segment("1000 H100 GPUs"), "100"));
        assert!(!occurs(&segment("costs 1.5"), "1"));
    }

    #[test]
    fn an_implied_constant_needs_its_reason_nearby() {
        let notes = "100M URLs a day, 500 bytes each, is 18.25TB a year.";
        assert_eq!(one("100M * 365 * 500 bytes", "18.25TB", "bytes", notes).len(), 1);
        let no_year = "100M URLs a day, 500 bytes each, is 18.25TB in total.";
        assert!(one("100M * 365 * 500 bytes", "18.25TB", "bytes", no_year).is_empty());
        assert!(one("100M / 86400", "1157", "count", "100M a day is 1157 at peak.").is_empty());
        assert_eq!(one("100M / 86400", "1157", "count", "100M a day is 1157 per sec.").len(), 1);
    }

    #[test]
    fn a_dash_across_words_and_a_decimal_comma_are_declined() {
        assert!(scan("5 users - 3 = 2").is_empty());
        assert_eq!(scan("100 - 0.5 = 99.5").calcs.len(), 1);
        assert_eq!(ev("1,5 * 2"), None, "a decimal comma is not a thousands separator");
        assert_eq!(ev("1,500 * 2"), Some(3000.0));
        assert!(scan("1,5 * 2 = 3").is_empty());
    }

    #[test]
    fn a_baseline_needs_the_word_accuracy_where_the_accuracy_is() {
        let notes = "Positives are 0.5%, recall 90%, precision 40%.";
        let raw = r#"{"positive_rate": "0.5%", "accuracy": "90%"}"#;
        assert!(Checks::from_extraction(raw, notes, Wanted::Baseline).is_empty());
        // And a rate from the accuracy sentence is not a rate.
        let notes = "Model accuracy is 95% with 0.5% positives.";
        let raw = r#"{"positive_rate": "0.5%", "accuracy": "95%"}"#;
        assert!(Checks::from_extraction(raw, notes, Wanted::Baseline).is_empty());
    }

    #[test]
    fn operands_keeps_only_the_sum() {
        assert_eq!(operands("100M new URLs/day * 365 days * 500B").as_deref(), Some("100M * 365 * 500B"));
        assert_eq!(operands("20K x 3").as_deref(), Some("20K * 3"));
        assert_eq!(operands("1e6 * 5").as_deref(), Some("1e6 * 5"));
        assert_eq!(operands("assume 100M URLs, 500 bytes each, so 100M * 500 bytes").as_deref(), Some("100M * 500 bytes"));
        assert_eq!(operands("2B users, 10% daily active"), None, "no operator, no sum");
        assert_eq!(operands("100 URLs/day"), None, "a per-slash is not division");
        assert_eq!(operands("(100M * 365) * 500B"), None);
        assert_eq!(operands(&"(".repeat(5000)), None);
    }

    #[test]
    fn garbage_in_means_no_checks() {
        for raw in ["", "no json here", "{", "{\"calculations\": 7}", "[1,2]", "{\"calculations\": [{\"expression\": 5}]}"] {
            assert!(Checks::from_extraction(raw, NOTES, Wanted::Calculations).is_empty(), "{raw}");
            assert!(Checks::from_extraction(raw, NOTES, Wanted::Baseline).is_empty(), "{raw}");
        }
        assert_eq!(Checks::default().instruction(), "");
        assert_eq!(Checks::default().report(), "");
    }

    #[test]
    fn a_fenced_answer_still_parses() {
        let raw = "```json\n{\"calculations\": [{\"expression\": \"100M * 365 * 500B\", \"claimed\": \"18GB\", \"unit\": \"bytes\"}]}\n```";
        assert_eq!(calcs(raw, NOTES).len(), 1);
    }

    #[test]
    fn the_baseline_sentence_is_finished_by_code() {
        let notes = "Spam is 0.5% of all messages. Our model scores 95% accuracy.";
        let raw = r#"{"calculations": [], "positive_rate": "0.5%", "accuracy": "95%"}"#;
        let checks = Checks::from_extraction(raw, notes, Wanted::Baseline);
        let s = checks.report();
        assert!(s.starts_with("Always-no baseline: 100 - 0.5 = 99.5%. Your accuracy: 95%."), "{s}");
        assert!(s.contains("4.5 points below"), "{s}");
        assert!(checks.instruction().contains("That is settled"));
    }

    #[test]
    fn a_baseline_needs_both_percentages_from_the_notes() {
        let notes = "Spam is 0.5% of all messages. Our model scores 95% accuracy.";
        // The fraction slip the model makes on its own: 0.5% handed back as 0.005.
        let fraction = r#"{"positive_rate": "0.005", "accuracy": "95%"}"#;
        assert!(Checks::from_extraction(fraction, notes, Wanted::Baseline).is_empty());
        let invented = r#"{"positive_rate": "2%", "accuracy": "95%"}"#;
        assert!(Checks::from_extraction(invented, notes, Wanted::Baseline).is_empty());
        let missing = r#"{"positive_rate": "0.5%", "accuracy": ""}"#;
        assert!(Checks::from_extraction(missing, notes, Wanted::Baseline).is_empty());
    }

    #[test]
    fn a_majority_positive_class_flips_the_trivial_model() {
        let b = Baseline { positive_rate: 70.0, accuracy: 90.0 };
        assert!(b.sentence().starts_with("Always-yes baseline: 70%."), "{}", b.sentence());
        assert!(b.sentence().contains("20 points above"));
        let tie = Baseline { positive_rate: 5.0, accuracy: 95.0 }.sentence();
        assert!(tie.contains("exactly the baseline") && !tie.contains("0 points"), "{tie}");
    }

    #[test]
    fn the_stage_gets_only_its_half() {
        let notes = "Spam is 0.5% of messages. The model is 95% accurate. 100M * 365 * 500B = 18GB a year";
        let raw = r#"{"calculations": [{"expression": "100M * 365 * 500B", "claimed": "18GB", "unit": "bytes"}],
                      "positive_rate": "0.5%", "accuracy": "95%"}"#;
        let est = Checks::from_extraction(raw, notes, Wanted::Calculations);
        assert!(est.baseline.is_none() && est.calcs.len() == 1);
        let ml = Checks::from_extraction(raw, notes, Wanted::Baseline);
        assert!(ml.calcs.is_empty() && ml.baseline.is_some());
    }

    #[test]
    fn human_units() {
        assert_eq!(human(18e9, true), "18 GB");
        assert_eq!(human(500.0, true), "500 bytes");
        assert_eq!(human(36.5e9, false), "36.5 billion");
        assert_eq!(human(1157.407, false), "1157.41");
        assert_eq!(sig(1013.9, 2), "1000");
        assert_eq!(sig(4.56, 2), "4.6");
        assert_eq!(dec(0.005), "0.005");
        assert_eq!(dec(0.00123), "0.0012");
        assert_eq!(dec(0.0), "0");
    }
}
