//! The type checker: turns a parsed formula into [`Ir`] with property references resolved,
//! variables as stack slots and every call checked against its signature (spec 15.5, 15.6).
//! The tree is at most `MAX_DEPTH` deep, so the recursion is bounded.

use super::syntax::{BinaryOp, Node, NodeKind};
use super::value::Val;
use super::{FormulaError, FormulaType as T, Span};

macro_rules! functions {
    ($($variant:ident => $name:literal),* $(,)?) => {
        /// A built-in function (`if`, `ifs`, `let`, `lets` and `prop` compile to their own nodes).
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Func { $($variant),* }

        impl Func {
            fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)*
                    _ => None,
                }
            }

            pub const fn name(self) -> &'static str {
                match self { $(Self::$variant => $name),* }
            }
        }
    };
}

functions! {
    Empty => "empty", Equal => "equal", Unequal => "unequal",
    Length => "length", Substring => "substring", Contains => "contains", Lower => "lower",
    Upper => "upper", Trim => "trim", Replace => "replace", ReplaceAll => "replaceAll",
    Repeat => "repeat", Format => "format", Split => "split", Join => "join", Link => "link",
    Add => "add", Subtract => "subtract", Multiply => "multiply", Divide => "divide", Mod => "mod",
    Pow => "pow", Abs => "abs", Round => "round", Ceil => "ceil", Floor => "floor",
    Sqrt => "sqrt", Min => "min", Max => "max", Sum => "sum", Mean => "mean",
    Median => "median", Sign => "sign", ToNumber => "toNumber", Pi => "pi", E => "e",
    Now => "now", Today => "today", DateAdd => "dateAdd", DateSubtract => "dateSubtract",
    DateBetween => "dateBetween", DateStart => "dateStart", DateEnd => "dateEnd",
    DateRange => "dateRange", FormatDate => "formatDate", ParseDate => "parseDate",
    Year => "year", Month => "month", Date => "date", Day => "day", Hour => "hour",
    Minute => "minute", Week => "week", Timestamp => "timestamp",
    FromTimestamp => "fromTimestamp",
    Name => "name", Email => "email",
    At => "at", First => "first", Last => "last", Slice => "slice", Concat => "concat",
    Sort => "sort", Reverse => "reverse", Unique => "unique", Includes => "includes",
    Find => "find", FindIndex => "findIndex", Filter => "filter", Some => "some",
    Every => "every", Map => "map", Flat => "flat",
    Id => "id",
}

/// Date units of `dateAdd`, `dateSubtract` and `dateBetween`.
pub const UNITS: &[&str] = &[
    "years", "quarters", "months", "weeks", "days", "hours", "minutes",
];

#[derive(Clone, Debug)]
pub enum Ir {
    Const(Val),
    List(Vec<Ir>),
    /// The value of the property at this index of the database's properties.
    Prop(usize),
    /// A variable: a slot of the local stack, counted from the bottom.
    Local(usize),
    Call(Func, Vec<Ir>),
    /// `map`, `filter` and friends: the body runs with `current` and `index` pushed.
    Lambda(Func, Box<Ir>, Box<Ir>),
    /// `let`: the value is pushed while the body runs.
    Let(Box<Ir>, Box<Ir>),
    If(Box<Ir>, Box<Ir>, Box<Ir>),
    Negate(Box<Ir>),
    Not(Box<Ir>),
    Binary(BinaryOp, Box<Ir>, Box<Ir>),
    /// Each operator in turn on the value so far and its operand.
    Chain(Box<Ir>, Vec<(BinaryOp, Ir)>),
}

/// A property as the checker sees it.
pub struct PropInfo {
    /// The formula type, or why a formula cannot read it.
    pub ty: Result<T, String>,
}

pub struct Checker<'a> {
    pub props: &'a [PropInfo],
    /// Resolves the text of `prop("…")` to a property index.
    pub resolve: &'a dyn Fn(&str) -> Result<usize, String>,
    scope: Vec<(String, T)>,
}

type Typed = (Ir, T);

fn err(message: impl Into<String>, span: Span) -> FormulaError {
    FormulaError::new(message, span)
}

fn list(item: T) -> T {
    T::List(Box::new(item))
}

fn item_of(ty: &T) -> Option<T> {
    match ty {
        T::List(item) => Some((**item).clone()),
        T::Unknown => Some(T::Unknown),
        _ => None,
    }
}

impl<'a> Checker<'a> {
    pub fn new(props: &'a [PropInfo], resolve: &'a dyn Fn(&str) -> Result<usize, String>) -> Self {
        Self {
            props,
            resolve,
            scope: Vec::new(),
        }
    }

    pub fn check(&mut self, node: &Node) -> Result<Typed, FormulaError> {
        let span = node.span;
        Ok(match &node.kind {
            NodeKind::Number(number) => (Ir::Const(Val::Number(*number)), T::Number),
            NodeKind::Text(text) => (Ir::Const(Val::Text(text.clone())), T::Text),
            NodeKind::Bool(flag) => (Ir::Const(Val::Bool(*flag)), T::Boolean),
            NodeKind::List(items) => {
                let mut ty = T::Unknown;
                let mut irs = Vec::with_capacity(items.len());
                for item in items {
                    let (ir, item_ty) = self.check(item)?;
                    ty = ty.unify(&item_ty).ok_or_else(|| {
                        err(
                            format!(
                                "A list holds one type: found {} and {}.",
                                ty.name(),
                                item_ty.name()
                            ),
                            item.span,
                        )
                    })?;
                    irs.push(ir);
                }
                (Ir::List(irs), list(ty))
            }
            NodeKind::Name(name) => {
                let slot = self
                    .scope
                    .iter()
                    .rposition(|(local, _)| local == name)
                    .ok_or_else(|| {
                        let hint = match name.as_str() {
                            "current" | "index" => {
                                format!("`{name}` works only inside map(), filter() and friends.")
                            }
                            _ => {
                                format!("Unknown name `{name}`. Use prop(\"Name\") for a property.")
                            }
                        };
                        err(hint, span)
                    })?;
                (Ir::Local(slot), self.scope[slot].1.clone())
            }
            NodeKind::Negate(inner) => {
                let ir = self.expect(inner, &T::Number, "`-`")?;
                (Ir::Negate(Box::new(ir)), T::Number)
            }
            NodeKind::Not(inner) => {
                let ir = self.expect(inner, &T::Boolean, "`not`")?;
                (Ir::Not(Box::new(ir)), T::Boolean)
            }
            NodeKind::Ternary(test, then, other) => self.branch(test, then, other, "`?`")?,
            NodeKind::Binary(op, left, right) => {
                let (left_ir, left_ty) = self.check(left)?;
                let (right_ir, right_ty) = self.check(right)?;
                let ty = binary_type(*op, &left_ty, &right_ty, span)?;
                (Ir::Binary(*op, Box::new(left_ir), Box::new(right_ir)), ty)
            }
            NodeKind::Chain(first, rest) => {
                let (first_ir, mut ty) = self.check(first)?;
                let mut steps = Vec::with_capacity(rest.len());
                for (op, right) in rest {
                    let (right_ir, right_ty) = self.check(right)?;
                    ty = binary_type(*op, &ty, &right_ty, first.span.to(right.span))?;
                    steps.push((*op, right_ir));
                }
                (Ir::Chain(Box::new(first_ir), steps), ty)
            }
            NodeKind::Call {
                name,
                name_span,
                args,
            } => self.call(name, *name_span, args, span)?,
        })
    }

    fn expect(&mut self, node: &Node, ty: &T, what: &str) -> Result<Ir, FormulaError> {
        let (ir, found) = self.check(node)?;
        if found.unify(ty).is_none() {
            return Err(err(
                format!("{what} needs {}, found {}.", ty.article(), found.article()),
                node.span,
            ));
        }
        Ok(ir)
    }

    fn branch(
        &mut self,
        test: &Node,
        then: &Node,
        other: &Node,
        what: &str,
    ) -> Result<Typed, FormulaError> {
        let test = self.expect(test, &T::Boolean, &format!("The condition of {what}"))?;
        let (then_ir, then_ty) = self.check(then)?;
        let (other_ir, other_ty) = self.check(other)?;
        let ty = then_ty.unify(&other_ty).ok_or_else(|| {
            err(
                format!(
                    "Both results of {what} need the same type: found {} and {}.",
                    then_ty.name(),
                    other_ty.name()
                ),
                other.span,
            )
        })?;
        Ok((
            Ir::If(Box::new(test), Box::new(then_ir), Box::new(other_ir)),
            ty,
        ))
    }

    fn call(
        &mut self,
        name: &str,
        name_span: Span,
        args: &[Node],
        span: Span,
    ) -> Result<Typed, FormulaError> {
        let count = |min: usize, max: usize| -> Result<(), FormulaError> {
            if (min..=max).contains(&args.len()) {
                return Ok(());
            }
            let wanted = match (min, max) {
                (min, max) if min == max => format!("{min}"),
                (min, usize::MAX) => format!("at least {min}"),
                (min, max) => format!("{min} to {max}"),
            };
            Err(err(
                format!(
                    "`{name}` takes {wanted} argument{}, found {}.",
                    if wanted == "1" { "" } else { "s" },
                    args.len()
                ),
                span,
            ))
        };
        match name {
            "prop" => {
                count(1, 1)?;
                let NodeKind::Text(reference) = &args[0].kind else {
                    return Err(err(
                        "prop() takes the name of a property in quotes.",
                        args[0].span,
                    ));
                };
                let index =
                    (self.resolve)(reference).map_err(|message| err(message, args[0].span))?;
                let ty = self.props[index]
                    .ty
                    .clone()
                    .map_err(|message| err(message, args[0].span))?;
                return Ok((Ir::Prop(index), ty));
            }
            "if" => {
                count(3, 3)?;
                return self.branch(&args[0], &args[1], &args[2], "`if`");
            }
            "ifs" => {
                count(2, usize::MAX)?;
                return self.ifs(args);
            }
            "let" => {
                count(3, 3)?;
                return self.lets(args);
            }
            "lets" => {
                count(3, usize::MAX)?;
                if args.len() % 2 == 0 {
                    return Err(err(
                        "lets() takes pairs of a name and a value, then the result.",
                        span,
                    ));
                }
                return self.lets(args);
            }
            _ => {}
        }
        let func = Func::from_name(name)
            .ok_or_else(|| err(format!("Unknown function `{name}`."), name_span))?;
        if matches!(
            func,
            Func::Find | Func::FindIndex | Func::Filter | Func::Some | Func::Every | Func::Map
        ) {
            count(2, 2)?;
            return self.lambda(func, &args[0], &args[1]);
        }
        let mut typed = Vec::with_capacity(args.len());
        for arg in args {
            typed.push(self.check(arg)?);
        }
        let types: Vec<T> = typed.iter().map(|(_, ty)| ty.clone()).collect();
        let spans: Vec<Span> = args.iter().map(|arg| arg.span).collect();
        let result = self.signature(func, &types, &spans, span, &count)?;
        if let (Func::DateAdd | Func::DateSubtract | Func::DateBetween, Some(NodeKind::Text(unit))) =
            (func, args.get(2).map(|arg| &arg.kind))
            && !UNITS.contains(&unit.as_str())
        {
            return Err(err(
                format!("Unknown unit \"{unit}\". Use one of: {}.", UNITS.join(", ")),
                args[2].span,
            ));
        }
        Ok((
            Ir::Call(func, typed.into_iter().map(|(ir, _)| ir).collect()),
            result,
        ))
    }

    fn ifs(&mut self, args: &[Node]) -> Result<Typed, FormulaError> {
        let pairs = args.len() / 2;
        let (mut tail, mut ty) = match args.len() % 2 {
            1 => self.check(&args[args.len() - 1])?,
            _ => (Ir::Const(Val::Empty), T::Unknown),
        };
        let mut branches = Vec::with_capacity(pairs);
        for pair in 0..pairs {
            let test = self.expect(&args[pair * 2], &T::Boolean, "The condition of `ifs`")?;
            let (value, value_ty) = self.check(&args[pair * 2 + 1])?;
            ty = ty.unify(&value_ty).ok_or_else(|| {
                err(
                    format!(
                        "Every result of `ifs` needs the same type: found {} and {}.",
                        ty.name(),
                        value_ty.name()
                    ),
                    args[pair * 2 + 1].span,
                )
            })?;
            branches.push((test, value));
        }
        for (test, value) in branches.into_iter().rev() {
            tail = Ir::If(Box::new(test), Box::new(value), Box::new(tail));
        }
        Ok((tail, ty))
    }

    /// `let(name, value, result)` and `lets(name, value, …, result)`.
    fn lets(&mut self, args: &[Node]) -> Result<Typed, FormulaError> {
        let pairs = args.len() / 2;
        let mut values = Vec::with_capacity(pairs);
        for pair in 0..pairs {
            let NodeKind::Name(name) = &args[pair * 2].kind else {
                return Err(err(
                    "Expected a variable name (letters, no quotes).",
                    args[pair * 2].span,
                ));
            };
            let (ir, ty) = self.check(&args[pair * 2 + 1])?;
            self.scope.push((name.clone(), ty));
            values.push(ir);
        }
        let body = self.check(&args[args.len() - 1]);
        self.scope.truncate(self.scope.len() - pairs);
        let (mut ir, ty) = body?;
        for value in values.into_iter().rev() {
            ir = Ir::Let(Box::new(value), Box::new(ir));
        }
        Ok((ir, ty))
    }

    fn lambda(&mut self, func: Func, items: &Node, body: &Node) -> Result<Typed, FormulaError> {
        let (list_ir, list_ty) = self.check(items)?;
        let item = item_of(&list_ty).ok_or_else(|| {
            err(
                format!(
                    "`{}` needs a list, found {}.",
                    func.name(),
                    list_ty.article()
                ),
                items.span,
            )
        })?;
        self.scope.push(("current".to_owned(), item.clone()));
        self.scope.push(("index".to_owned(), T::Number));
        let checked = self.check(body);
        self.scope.truncate(self.scope.len() - 2);
        let (body_ir, body_ty) = checked?;
        if func != Func::Map && body_ty.unify(&T::Boolean).is_none() {
            return Err(err(
                format!(
                    "The test of `{}` needs true/false, found {}.",
                    func.name(),
                    body_ty.article()
                ),
                body.span,
            ));
        }
        let ty = match func {
            Func::Find => item,
            Func::FindIndex => T::Number,
            Func::Filter => list(item),
            Func::Map => list(body_ty),
            _ => T::Boolean,
        };
        Ok((Ir::Lambda(func, Box::new(list_ir), Box::new(body_ir)), ty))
    }

    /// The result type of `func` called with `types`, or the error at the first bad argument.
    fn signature(
        &self,
        func: Func,
        types: &[T],
        spans: &[Span],
        span: Span,
        count: &dyn Fn(usize, usize) -> Result<(), FormulaError>,
    ) -> Result<T, FormulaError> {
        let name = func.name();
        let arg = |at: usize, want: &T| -> Result<(), FormulaError> {
            if types[at].unify(want).is_some() {
                return Ok(());
            }
            Err(err(
                format!(
                    "Argument {} of `{name}` needs {}, found {}.",
                    at + 1,
                    want.article(),
                    types[at].article()
                ),
                spans[at],
            ))
        };
        let list_arg = |at: usize| -> Result<T, FormulaError> {
            item_of(&types[at]).ok_or_else(|| {
                err(
                    format!(
                        "Argument {} of `{name}` needs a list, found {}.",
                        at + 1,
                        types[at].article()
                    ),
                    spans[at],
                )
            })
        };
        let all = |want: &T| -> Result<(), FormulaError> {
            (0..types.len()).try_for_each(|at| arg(at, want))
        };
        Ok(match func {
            Func::Empty | Func::Format => {
                count(1, 1)?;
                if func == Func::Empty {
                    T::Boolean
                } else {
                    T::Text
                }
            }
            Func::Equal | Func::Unequal => {
                count(2, 2)?;
                if types[0].unify(&types[1]).is_none() {
                    return Err(err(
                        format!(
                            "`{name}` cannot compare {} and {}.",
                            types[0].name(),
                            types[1].name()
                        ),
                        span,
                    ));
                }
                T::Boolean
            }
            Func::Length => {
                count(1, 1)?;
                if item_of(&types[0]).is_none() {
                    arg(0, &T::Text)?;
                }
                T::Number
            }
            Func::Substring => {
                count(2, 3)?;
                arg(0, &T::Text)?;
                (1..types.len()).try_for_each(|at| arg(at, &T::Number))?;
                T::Text
            }
            Func::Contains => {
                count(2, 2)?;
                all(&T::Text)?;
                T::Boolean
            }
            Func::Lower | Func::Upper | Func::Trim => {
                count(1, 1)?;
                all(&T::Text)?;
                T::Text
            }
            Func::Replace | Func::ReplaceAll => {
                count(3, 3)?;
                all(&T::Text)?;
                T::Text
            }
            Func::Repeat => {
                count(2, 2)?;
                arg(0, &T::Text)?;
                arg(1, &T::Number)?;
                T::Text
            }
            Func::Split => {
                count(1, 2)?;
                all(&T::Text)?;
                list(T::Text)
            }
            Func::Join => {
                count(1, 2)?;
                list_arg(0)?;
                if types.len() == 2 {
                    arg(1, &T::Text)?;
                }
                T::Text
            }
            Func::Link => {
                count(2, 2)?;
                all(&T::Text)?;
                T::Text
            }
            Func::Add => {
                count(2, 2)?;
                if types[0].unify(&T::Text).is_some() && types[1].unify(&T::Text).is_some() {
                    T::Text
                } else {
                    all(&T::Number)?;
                    T::Number
                }
            }
            Func::Subtract | Func::Multiply | Func::Divide | Func::Mod | Func::Pow => {
                count(2, 2)?;
                all(&T::Number)?;
                T::Number
            }
            Func::Abs | Func::Ceil | Func::Floor | Func::Sqrt | Func::Sign => {
                count(1, 1)?;
                all(&T::Number)?;
                T::Number
            }
            Func::Round => {
                count(1, 2)?;
                all(&T::Number)?;
                T::Number
            }
            Func::Min | Func::Max | Func::Sum | Func::Mean | Func::Median => {
                count(1, usize::MAX)?;
                for at in 0..types.len() {
                    if types[at].unify(&list(T::Number)).is_none() {
                        arg(at, &T::Number)?;
                    }
                }
                T::Number
            }
            Func::ToNumber => {
                count(1, 1)?;
                T::Number
            }
            Func::Pi | Func::E => {
                count(0, 0)?;
                T::Number
            }
            Func::Now | Func::Today => {
                count(0, 0)?;
                T::Date
            }
            Func::DateAdd | Func::DateSubtract => {
                count(3, 3)?;
                arg(0, &T::Date)?;
                arg(1, &T::Number)?;
                arg(2, &T::Text)?;
                T::Date
            }
            Func::DateBetween => {
                count(3, 3)?;
                arg(0, &T::Date)?;
                arg(1, &T::Date)?;
                arg(2, &T::Text)?;
                T::Number
            }
            Func::DateStart | Func::DateEnd => {
                count(1, 1)?;
                all(&T::Date)?;
                T::Date
            }
            Func::DateRange => {
                count(2, 2)?;
                all(&T::Date)?;
                T::Date
            }
            Func::FormatDate => {
                count(2, 2)?;
                arg(0, &T::Date)?;
                arg(1, &T::Text)?;
                T::Text
            }
            Func::ParseDate => {
                count(1, 1)?;
                all(&T::Text)?;
                T::Date
            }
            Func::Year
            | Func::Month
            | Func::Date
            | Func::Day
            | Func::Hour
            | Func::Minute
            | Func::Week
            | Func::Timestamp => {
                count(1, 1)?;
                all(&T::Date)?;
                T::Number
            }
            Func::FromTimestamp => {
                count(1, 1)?;
                all(&T::Number)?;
                T::Date
            }
            Func::Name | Func::Email => {
                count(1, 1)?;
                if types[0].unify(&list(T::Person)).is_some() && types[0] != T::Unknown {
                    list(T::Text)
                } else {
                    arg(0, &T::Person)?;
                    T::Text
                }
            }
            Func::Id => {
                count(0, 1)?;
                if !types.is_empty() {
                    arg(0, &T::Page)?;
                }
                T::Text
            }
            Func::At => {
                count(2, 2)?;
                let item = list_arg(0)?;
                arg(1, &T::Number)?;
                item
            }
            Func::First | Func::Last => {
                count(1, 1)?;
                list_arg(0)?
            }
            Func::Slice => {
                count(2, 3)?;
                list_arg(0)?;
                (1..types.len()).try_for_each(|at| arg(at, &T::Number))?;
                types[0].clone()
            }
            Func::Concat => {
                count(1, usize::MAX)?;
                let mut ty = list(T::Unknown);
                for at in 0..types.len() {
                    list_arg(at)?;
                    ty = ty.unify(&types[at]).ok_or_else(|| {
                        err(
                            format!(
                                "`concat` joins lists of one type: found {} and {}.",
                                ty.name(),
                                types[at].name()
                            ),
                            spans[at],
                        )
                    })?;
                }
                ty
            }
            Func::Sort => {
                count(1, 1)?;
                let item = list_arg(0)?;
                if !matches!(
                    item,
                    T::Number | T::Text | T::Boolean | T::Date | T::Unknown
                ) {
                    return Err(err(
                        format!("`sort` cannot order a list of {}.", item.name()),
                        spans[0],
                    ));
                }
                types[0].clone()
            }
            Func::Reverse | Func::Unique => {
                count(1, 1)?;
                list_arg(0)?;
                types[0].clone()
            }
            Func::Includes => {
                count(2, 2)?;
                let item = list_arg(0)?;
                if item.unify(&types[1]).is_none() {
                    return Err(err(
                        format!(
                            "`includes` looks for {} in this list, found {}.",
                            item.article(),
                            types[1].article()
                        ),
                        spans[1],
                    ));
                }
                T::Boolean
            }
            Func::Flat => {
                count(1, 1)?;
                match list_arg(0)? {
                    T::List(inner) => T::List(inner),
                    _ => types[0].clone(),
                }
            }
            Func::Find | Func::FindIndex | Func::Filter | Func::Some | Func::Every | Func::Map => {
                unreachable!("checked as a lambda")
            }
        })
    }
}

fn binary_type(op: BinaryOp, left_ty: &T, right_ty: &T, span: Span) -> Result<T, FormulaError> {
    let symbol = op.symbol();
    let both = |ty: &T| left_ty.unify(ty).is_some() && right_ty.unify(ty).is_some();
    let ty = match op {
        BinaryOp::Add if both(&T::Number) => T::Number,
        BinaryOp::Add if both(&T::Text) => T::Text,
        BinaryOp::Add => {
            let hint = if [left_ty, right_ty].contains(&&T::Text) {
                " Use format() to make text."
            } else {
                ""
            };
            return Err(err(
                format!(
                    "`+` cannot add {} and {}.{hint}",
                    left_ty.name(),
                    right_ty.name()
                ),
                span,
            ));
        }
        BinaryOp::Subtract
        | BinaryOp::Multiply
        | BinaryOp::Divide
        | BinaryOp::Remainder
        | BinaryOp::Power => {
            if !both(&T::Number) {
                return Err(err(
                    format!(
                        "`{symbol}` needs numbers, found {} and {}.",
                        left_ty.name(),
                        right_ty.name()
                    ),
                    span,
                ));
            }
            T::Number
        }
        BinaryOp::And | BinaryOp::Or => {
            if !both(&T::Boolean) {
                return Err(err(
                    format!(
                        "`{symbol}` needs true/false values, found {} and {}.",
                        left_ty.name(),
                        right_ty.name()
                    ),
                    span,
                ));
            }
            T::Boolean
        }
        BinaryOp::Equal | BinaryOp::NotEqual => {
            if left_ty.unify(right_ty).is_none() {
                return Err(err(
                    format!(
                        "`{symbol}` cannot compare {} and {}.",
                        left_ty.name(),
                        right_ty.name()
                    ),
                    span,
                ));
            }
            T::Boolean
        }
        BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
            let ordered = left_ty
                .unify(&right_ty)
                .is_some_and(|ty| matches!(ty, T::Number | T::Text | T::Date | T::Unknown));
            if !ordered {
                return Err(err(
                    format!(
                        "`{symbol}` compares two numbers, texts or dates, found {} and {}.",
                        left_ty.name(),
                        right_ty.name()
                    ),
                    span,
                ));
            }
            T::Boolean
        }
    };
    Ok(ty)
}
