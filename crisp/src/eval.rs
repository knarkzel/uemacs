use crate::*;
use std::collections::HashMap;

// Eval helpers
#[throws]
fn expr_to_number(expr: &Expr) -> i32 {
    match expr {
        Expr::Constant(Atom::Number(it)) => *it,
        _ => bail!("Invalid number passed: {}", expr),
    }
}

fn number_to_expr(number: i32) -> Expr {
    Expr::Constant(Atom::Number(number))
}

#[throws]
fn expr_to_boolean(expr: &Expr) -> bool {
    match expr {
        Expr::Nil => false,
        Expr::Quote(items) if items.len() == 0 => false,
        _ => true,
    }
}

fn boolean_to_expr(boolean: bool) -> Expr {
    match boolean {
        false => Expr::Nil,
        true => Expr::Constant(Atom::Symbol(String::from("T"))),
    }
}

#[throws]
fn numbers(tail: &[Expr]) -> impl Iterator<Item = i32> {
    tail.iter()
        .map(expr_to_number)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
}

#[throws]
fn booleans(tail: &[Expr]) -> impl Iterator<Item = bool> {
    tail.iter()
        .map(expr_to_boolean)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
}

fn car<T>(tail: &[T]) -> Option<&T> {
    tail.first()
}

fn cdr<T>(tail: &[T]) -> Option<&[T]> {
    match tail.len() > 0 {
        true => Some(&tail[1..]),
        false => None,
    }
}

// Curry lambda by recursively translating each variable into its parameter
#[throws]
fn curry(expr: Expr, left: &[Expr], right: &[Expr], marked: &mut [bool]) -> Expr {
    let mut single = |expr| -> Result<Expr, Error> { curry(expr, left, right, marked) };
    match expr {
        // Handle variable
        symbol if let Some(index) = left.iter().position(|it| *it == symbol) => {
            marked[index] = true;
            match right.get(index) {
                Some(parameter) => parameter.clone(),
                None => bail!("Index out of bounds"),
            }
        },
        // Handle other forms
        Expr::Call(head, tail) => {
            let head = Box::new(single(*head)?);
            let tail = tail.into_iter().map(single).collect::<Result<Vec<_>, _>>()?;
            Expr::Call(head, tail)
        },
        Expr::If(predicate, then, otherwise) => Expr::If(Box::new(single(*predicate)?), Box::new(single(*then)?), otherwise.and_then(|it| single(*it).ok().map(Box::new))),
        it => it,
    }
}

// Macros
macro_rules! logic {
	($tail:ident => $a:ident $op:tt $b:ident) => {
		boolean_to_expr($tail.windows(2).all(|it| match (&it[0], &it[1]) {
            (Expr::Constant(Atom::Number($a)), Expr::Constant(Atom::Number($b))) => {
                $a $op $b
            }
            _ => false,
        }))
	};
}

// Context
#[derive(Default)]
pub struct Context {
    environment: HashMap<String, Expr>,
}

impl Context {
    pub fn eval(&mut self, mut expr: Expr) -> Result<Expr, Error> {
        loop {
            match expr {
                Expr::Constant(Atom::Symbol(symbol)) => match self.environment.get(&symbol) {
                    Some(expr) => return Ok(expr.clone()),
                    None => bail!("Invalid variable: {symbol}"),
                },
                Expr::Constant(_) | Expr::Quote(_) => return Ok(expr),
                Expr::Let(items) => {
                    for item in items {
                        match item.0 {
                            Atom::Symbol(name) => {
                                let expr = self.eval(*item.1)?;
                                self.environment.insert(name, expr);
                            }
                            _ => bail!("Expected symbol, found following: {}", item.0),
                        }
                    }
                    return Ok(Expr::Nil);
                }
                Expr::If(predicate, then, otherwise) => {
                    let predicate = self.eval(*predicate)?;
                    if expr_to_boolean(&predicate)? {
                        expr = *then;
                        continue;
                    } else if let Some(branch) = otherwise {
                        expr = *branch;
                        continue;
                    } else {
                        bail!("No branches of predicate ran: {predicate}")
                    }
                }
                Expr::Call(head, tail) => {
                    let head = self.eval(*head)?;
                    let tail = tail
                        .into_iter()
                        .map(|it| self.eval(it))
                        .collect::<Result<Vec<_>, _>>()?;
                    match head {
                        Expr::Function(args, fexpr) => {
                            let mut marked = (0..args.len()).map(|_| false).collect::<Vec<_>>();
                            let body = curry(*fexpr, &args, &tail, &mut marked)?;
                            let args = args
                                .into_iter()
                                .zip(marked.into_iter())
                                .filter_map(|(it, marked)| if marked { None } else { Some(it) })
                                .collect::<Vec<_>>();
                            if args.len() == 0 {
                                expr = body;
                                continue;
                            } else {
                                return Ok(Expr::Function(args, Box::new(body)));
                            }
                        }
                        Expr::Constant(Atom::BuiltIn(built_in)) => {
                            return Ok(match built_in {
                                BuiltIn::Greater => logic!(tail => a > b),
                                BuiltIn::Less => logic!(tail => a < b),
                                BuiltIn::GreaterEqual => logic!(tail => a >= b),
                                BuiltIn::LessEqual => logic!(tail => a <= b),
                                BuiltIn::Plus => number_to_expr(numbers(&tail)?.sum()),
                                BuiltIn::Minus => match car(&tail).map(expr_to_number) {
                                    Some(Ok(car)) => number_to_expr(
                                        numbers(cdr(&tail).unwrap_or_default())?
                                            .fold(car, |a, b| a - b),
                                    ),
                                    _ => {
                                        bail!(
                                            "- expects one or more parameters, found {}",
                                            tail.len()
                                        )
                                    }
                                },
                                BuiltIn::Times => number_to_expr(numbers(&tail)?.product()),
                                BuiltIn::Equal => {
                                    boolean_to_expr(tail.windows(2).all(|it| it[0] == it[1]))
                                }
                                BuiltIn::NotEqual => {
                                    boolean_to_expr(tail.windows(2).all(|it| it[0] != it[1]))
                                }
                                BuiltIn::And => boolean_to_expr(booleans(&tail)?.all(|it| it)),
                                BuiltIn::Or => boolean_to_expr(booleans(&tail)?.any(|it| it)),
                                BuiltIn::Divide => match car(&tail).map(expr_to_number) {
                                    Some(Ok(car)) => number_to_expr(
                                        numbers(cdr(&tail).unwrap_or_default())?
                                            .fold(car, |a, b| a / b),
                                    ),
                                    _ => bail!(
                                        "/ expects 1 or more parameters, found {}",
                                        tail.len()
                                    ),
                                },
                                BuiltIn::Not => match (tail.len() == 1, car(&tail)) {
                                    (true, Some(car)) => boolean_to_expr(!expr_to_boolean(car)?),
                                    _ => bail!("! expects 1 parameter, got {}", tail.len()),
                                },
                            })
                        }
                        it => return Ok(it),
                    }
                }
                it => return Ok(it),
            }
        }
    }
}
