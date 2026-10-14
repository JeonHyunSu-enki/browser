#[repr(align(8))]
#[derive(Clone)]
pub struct Expression {
    value: Value,
}
#[derive(Clone)]
enum Value {
    Number(f64),
    Length(f64, f64),
    Sum(Box<Expression>, Box<Expression>),
    Product(Box<Expression>, Box<Expression>),
    Divide(Box<Expression>, Box<Expression>),
    Min(Vec<Expression>),
    Max(Vec<Expression>),
    Clamp(Vec<Expression>),
}
impl Expression {
    fn number(value: f64) -> Self {
        Self {
            value: Value::Number(value),
        }
    }
    fn length(px: f64, percent: f64) -> Self {
        Self {
            value: Value::Length(px, percent),
        }
    }
    fn scalar(&self) -> bool {
        match &self.value {
            Value::Number(_) => true,
            Value::Length(_, _) => false,
            Value::Sum(a, b) => a.scalar() && b.scalar(),
            Value::Product(a, b) => a.scalar() && b.scalar(),
            Value::Divide(a, b) => a.scalar() == b.scalar(),
            Value::Min(a) | Value::Max(a) | Value::Clamp(a) => a.iter().all(Self::scalar),
        }
    }
    pub fn evaluate(&self, basis: f64) -> f64 {
        match &self.value {
            Value::Number(v) => *v,
            Value::Length(px, p) => px + p * basis,
            Value::Sum(a, b) => a.evaluate(basis) + b.evaluate(basis),
            Value::Product(a, b) => a.evaluate(basis) * b.evaluate(basis),
            Value::Divide(a, b) => a.evaluate(basis) / b.evaluate(basis),
            Value::Min(a) => a
                .iter()
                .map(|a| a.evaluate(basis))
                .fold(f64::INFINITY, f64::min),
            Value::Max(a) => a
                .iter()
                .map(|a| a.evaluate(basis))
                .fold(f64::NEG_INFINITY, f64::max),
            Value::Clamp(a) => a[0]
                .evaluate(basis)
                .max(a[1].evaluate(basis).min(a[2].evaluate(basis))),
        }
    }
    pub fn depends_on_basis(&self) -> bool {
        match &self.value {
            Value::Length(_, p) => *p != 0.0,
            Value::Number(_) => false,
            Value::Sum(a, b) | Value::Product(a, b) | Value::Divide(a, b) => {
                a.depends_on_basis() || b.depends_on_basis()
            }
            Value::Min(a) | Value::Max(a) | Value::Clamp(a) => a.iter().any(Self::depends_on_basis),
        }
    }
}
pub fn evaluate(ptr: *const (), basis: f32) -> f32 {
    unsafe { (*(ptr as *const Expression)).evaluate(basis as f64) as f32 }
}
struct Parser<'a> {
    text: &'a [u8],
    at: usize,
    font: f64,
    root: f64,
    width: f64,
    height: f64,
    depth: u16,
}
impl Parser<'_> {
    fn ws(&mut self) {
        while self.at < self.text.len() && self.text[self.at].is_ascii_whitespace() {
            self.at += 1
        }
    }
    fn take(&mut self, c: u8) -> bool {
        self.ws();
        if self.text.get(self.at) == Some(&c) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn sum(&mut self) -> Result<Expression, ()> {
        let mut a = self.product()?;
        loop {
            self.ws();
            let sign = if self.take(b'+') {
                1.0
            } else if self.take(b'-') {
                -1.0
            } else {
                break;
            };
            let mut b = self.product()?;
            if a.scalar() != b.scalar() {
                return Err(());
            }
            if sign < 0.0 {
                b = Expression {
                    value: Value::Product(Box::new(Expression::number(-1.0)), Box::new(b)),
                }
            }
            a = Expression {
                value: Value::Sum(Box::new(a), Box::new(b)),
            }
        }
        Ok(a)
    }
    fn product(&mut self) -> Result<Expression, ()> {
        let mut a = self.atom()?;
        loop {
            if self.take(b'*') {
                let b = self.atom()?;
                if !a.scalar() && !b.scalar() {
                    return Err(());
                }
                a = Expression {
                    value: Value::Product(Box::new(a), Box::new(b)),
                }
            } else if self.take(b'/') {
                let b = self.atom()?;
                if b.evaluate(0.0) == 0.0 && !b.depends_on_basis() {
                    return Err(());
                }
                a = Expression {
                    value: Value::Divide(Box::new(a), Box::new(b)),
                }
            } else {
                break;
            }
        }
        Ok(a)
    }
    fn atom(&mut self) -> Result<Expression, ()> {
        self.ws();
        self.depth += 1;
        if self.depth > 128 {
            return Err(());
        }
        let result = self.atom_inner();
        self.depth -= 1;
        result
    }
    fn atom_inner(&mut self) -> Result<Expression, ()> {
        if self.take(b'(') {
            let x = self.sum()?;
            if !self.take(b')') {
                return Err(());
            }
            return Ok(x);
        }
        if self.take(b'-') {
            let x = self.atom()?;
            return Ok(Expression {
                value: Value::Product(Box::new(Expression::number(-1.0)), Box::new(x)),
            });
        }
        if self.take(b'+') {
            return self.atom();
        }
        self.ws();
        let start = self.at;
        while self
            .text
            .get(self.at)
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'-')
        {
            self.at += 1
        }
        if self.at > start {
            let name = std::str::from_utf8(&self.text[start..self.at])
                .map_err(|_| ())?
                .to_ascii_lowercase();
            if !self.take(b'(') {
                return Err(());
            }
            let mut values = vec![self.sum()?];
            while self.take(b',') {
                values.push(self.sum()?)
            }
            if !self.take(b')') || values.iter().any(|x| x.scalar() != values[0].scalar()) {
                return Err(());
            }
            return Ok(match name.as_str() {
                "calc" if values.len() == 1 => values.remove(0),
                "min" => Expression {
                    value: Value::Min(values),
                },
                "max" => Expression {
                    value: Value::Max(values),
                },
                "clamp" if values.len() == 3 => Expression {
                    value: Value::Clamp(values),
                },
                _ => return Err(()),
            });
        }
        let start = self.at;
        while self
            .text
            .get(self.at)
            .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
        {
            self.at += 1
        }
        if self
            .text
            .get(self.at)
            .is_some_and(|c| *c == b'e' || *c == b'E')
            && self
                .text
                .get(self.at + 1)
                .is_some_and(|c| c.is_ascii_digit() || *c == b'+' || *c == b'-')
        {
            self.at += 1;
            if self
                .text
                .get(self.at)
                .is_some_and(|c| *c == b'+' || *c == b'-')
            {
                self.at += 1
            }
            while self.text.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1
            }
        }
        if self.at == start {
            return Err(());
        }
        let v = std::str::from_utf8(&self.text[start..self.at])
            .map_err(|_| ())?
            .parse::<f64>()
            .map_err(|_| ())?;
        let start = self.at;
        while self
            .text
            .get(self.at)
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'%')
        {
            self.at += 1
        }
        let unit = std::str::from_utf8(&self.text[start..self.at])
            .map_err(|_| ())?
            .to_ascii_lowercase();
        Ok(match unit.as_str() {
            "" => Expression::number(v),
            "%" => Expression::length(0.0, v / 100.0),
            "px" => Expression::length(v, 0.0),
            "em" => Expression::length(v * self.font, 0.0),
            "rem" => Expression::length(v * self.root, 0.0),
            "vw" => Expression::length(v * self.width / 100.0, 0.0),
            "vh" => Expression::length(v * self.height / 100.0, 0.0),
            "vmin" => Expression::length(v * self.width.min(self.height) / 100.0, 0.0),
            "vmax" => Expression::length(v * self.width.max(self.height) / 100.0, 0.0),
            "pt" => Expression::length(v * 4.0 / 3.0, 0.0),
            "pc" => Expression::length(v * 16.0, 0.0),
            "in" => Expression::length(v * 96.0, 0.0),
            "cm" => Expression::length(v * 96.0 / 2.54, 0.0),
            "mm" => Expression::length(v * 96.0 / 25.4, 0.0),
            _ => return Err(()),
        })
    }
}
pub fn parse(text: &str, font: f32, root: f32, width: f32, height: f32) -> Result<Expression, ()> {
    let mut p = Parser {
        text: text.as_bytes(),
        at: 0,
        font: font as f64,
        root: root as f64,
        width: width as f64,
        height: height as f64,
        depth: 0,
    };
    let x = p.sum()?;
    p.ws();
    if p.at != p.text.len() || x.scalar() && x.evaluate(0.0) != 0.0 {
        return Err(());
    }
    Ok(x)
}
