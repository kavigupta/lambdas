use core::num;
// use crate::*;
// use crate::parse_expr::{curry_sexp,uncurry_sexp};
use std::collections::HashMap;
use std::fmt::{self, Formatter, Display, Debug};
use std::hash::Hash;
use std::ops::{Index, IndexMut, Range};
use serde::{Serialize, Deserialize};
use std::cmp::{min,max};


pub type Idx = usize;
pub const HOLE: usize = usize::MAX;


/// A node of an untyped lambda calculus expression compatible with `egg` but also used more widely throughout this crate.
/// Note that there is no domain associated with this object. This makes it easy to run compression on
/// domains that don't have semantics yet, makes it easy to add new prims (eg learned functions), etc.
/// You'll have to specify a domain when you execute the expression, type check it, etc, but you can easily do
/// that at the time through generics on those functions.
/// 
/// Variants:
/// * Var(i): "$i" a debruijn index variable
/// * IVar(i): "#i" a debruijn index variable used by inventions (advantage: readability of inventions + less shifting required)
/// * App([f, x]): f applied to x
/// * Lam([body]): lambda abstraction referred to through $i Vars
/// * Prim(Symbol): primitive (eg functions, constants, all nonvariable leaf nodes)
/// * Programs(Vec<Id>): list of root nodes of the programs. There's just one of these at the top of the program tree
/// 
/// Note there is no AppLam construct. This is because AppLams are represented through the `AppLam` struct when it comes
/// to invention-finding, and they don't belong in Lambda because they never actually show up within programs (theyre only
/// ever used in passing at the top level when constructing inventions) 
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Node where
{
    Var(i32), // db index ($i)
    Prim(egg::Symbol), // primitive (eg functions, constants, all nonvariable leaf nodes)
    App(Idx,Idx), // f, x
    Lam(Idx), // body
    IVar(i32)
}

/// An untyped lambda calculus expression, much like `egg::RecExpr` but with a public `nodes` field
/// and many attached functions. See `Lambda` for details on the individual nodes.
/// 
/// Creation:
/// * From<RecExpr> is implemented (and vis versa) for interop with `egg`
/// * Expr::new() directly constructs an Expr from a Vec<Lambda>
/// * Expr::prim(Symbol), Expr::app(Expr,Expr), etc let you construct Exprs from other Exprs
/// * Expr::from_curried(&str) parses from a curried string like (app (app + 3) 4)
/// * Expr::from_uncurried(&str) parses from an uncurried string like (+ 3 4)
/// 
/// Displaying an expression or subexpression:
/// * fmt::Display is implemented to return an uncurried string like (+ 3 4)
/// * Expr::to_curried_string(Option<Id>) returns a curried string like (app (app + 3) 4) rooted at the Id if given
/// * Expr::to_uncurried_string(Option<Id>) returns an uncurried string like (+ 3 4) rooted at the Id if given
/// * Expr::save() lets you save an image of the expr to a file
/// 
/// Creating a subexpression:
/// * Expr::cloned_subexpr(Id) returns the subexpression rooted at the Id. Generally you want to avoid this because
///   most methods can get by just fine by taking a parent Expr and a child Id without the need for all this cloning.
///   Importantly all Id indexing should be preserved just fine since this is implemented through truncating the underlying vector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExprSet {
    pub nodes: Vec<Node>,
    pub subtree_spans: Option<Vec<Range<Idx>>>,
    pub order: Order
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum Order {
    ChildFirst,
    ParentFirst,
    Any
}

#[derive(Clone,Copy)]
pub struct Expr<'a> {
    set: &'a ExprSet,
    idx: Idx 
}

pub struct ExprMut<'a> {
    set: &'a mut ExprSet,
    idx: Idx 
}

impl Index<Idx> for ExprSet {
    type Output = Node;
    #[inline(always)]
    fn index(&self, idx: Idx) -> &Self::Output {
        &self.nodes[idx]
    }
}
impl IndexMut<Idx> for ExprSet {
    #[inline(always)]
    fn index_mut(&mut self, idx: Idx) -> &mut Self::Output {
        &mut self.nodes[idx]
    }
}
impl Index<Range<Idx>> for ExprSet {
    type Output = [Node];
    #[inline(always)]
    fn index(&self, idx: Range<Idx>) -> &Self::Output {
        &self.nodes[idx]
    }
}
impl IndexMut<Range<Idx>> for ExprSet {
    #[inline(always)]
    fn index_mut(&mut self, idx: Range<Idx>) -> &mut Self::Output {
        &mut self.nodes[idx]
    }
}

impl ExprSet {
    fn empty(order: Order, subtree_spans: bool) -> ExprSet {
        let subtree_spans = if subtree_spans { Some(vec![]) } else { None };
        ExprSet { nodes: vec![], order, subtree_spans }
    }
    fn add(&mut self, node: Node) -> Idx {
        let idx = self.nodes.len();
        if let Some(spans) = &mut self.subtree_spans {
            let span = match node {
                Node::Var(_) | Node::Prim(_) | Node::IVar(_) => idx .. idx+1,
                Node::App(f, x) => min(min(spans[f].start,spans[x].start),idx) .. max(max(spans[f].end,spans[x].end),idx+1),
                Node::Lam(b) => min(spans[b].start,idx) .. max(spans[b].end,idx+1)
            };
            spans.push(span);
        }
        self.nodes.push(node);
        idx
    }
    fn get(&self, idx: Idx) -> Expr {
        Expr { set: self, idx }
    }
    fn get_mut(&mut self, idx: Idx) -> ExprMut {
        ExprMut { set: self, idx }
    }
}

impl<'a> Expr<'a> {
    fn get(&self, idx: Idx) -> Self {
        Self { set: self.set, idx }
    }
    fn node(&self) -> &Node {
        &self.set[self.idx]
    }
}

impl<'a> ExprMut<'a> {
    fn get(&mut self, idx: Idx) -> ExprMut {
        ExprMut { set: self.set, idx }
    }
    fn node(&mut self) -> &mut Node {
        &mut self.set[self.idx]
    }
}


/// printing a single node prints the operator
impl Display for Node {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Var(i) => write!(f, "${}", i),
            Self::Prim(p) => write!(f,"{}",p),
            Self::App(_,_) => write!(f,"app"),
            Self::Lam(_) => write!(f,"lam"),
            Self::IVar(i) => write!(f,"#{}",i),
        }
    }
}

impl<'a> Display for Expr<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn fmt_local(e: Expr, left_of_app: bool, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            if e.idx == HOLE {
                return write!(f,"??");
            }

            match e.node() {
                Node::Var(_) | Node::IVar(_) | Node::Prim(_) => write!(f,"{}", e.node()),
                Node::App(fun,x) => {
                    // if you are the left side of an application, and you are an application, you dont need parens
                    if !left_of_app { write!(f,"(")? }
                    fmt_local(e.get(*fun), true, f)?;
                    write!(f," ")?;
                    fmt_local(e.get(*x), false, f)?;
                    if !left_of_app { write!(f,")") } else { Ok(()) }
                },
                Node::Lam(b) => {
                    write!(f,"(lam ")?;
                    fmt_local(e.get(*b), false, f)?;
                    write!(f,")")
                }
            }
        }
        fmt_local(*self, false, f)
    }
}


impl ExprSet {
    fn parse_extend(&mut self, s_init: &str) -> Result<Idx,String> {
        let init_len = self.nodes.len();

        let mut s = s_init.trim();

        let mut items: Vec<Idx> = vec![];
        let mut items_of_depth: Vec<usize> = vec![]; // offsets[i] gives the number of items at depth i
        items_of_depth.push(0); // the zero paren depth

        while !s.trim().is_empty() {
            s = s.trim();
            let next =  s.chars().last().unwrap();
            if next == '(' {
                s = &s[..s.len()-1];
                let num_items = items_of_depth.pop().ok_or_else(||format!("ExprSet parse error: mismatched parens in: {}",s_init))?;
                if num_items == 0 {
                    continue
                }

                
                // now num_items >= 1. The following loop will only happen if num_items >= 2.
                // apply the last item to the second to last, etc
                for _ in 0..num_items-1 {
                    // println!("built an app inside");
                    let f: Idx = items.pop().unwrap();
                    let x: Idx = items.pop().unwrap();
                    items.push(self.add(Node::App(f, x)))
                }
                // then we simply leave that final result pushed on
                if let Some(num_items) = items_of_depth.last_mut() {
                    *num_items += 1;
                } else {
                    return Err(format!("ExprSet parse error: mismatched parens in: {}",s_init));
                }
                continue
            }
            if next == ')' {
                s = &s[..s.len()-1];
                items_of_depth.push(0);
                continue
            }
            // parse a space-separated word
            // println!("parsing with s: `{}`", s);
            let start = {
                let mut i = s.len()-1;
                loop {
                    if i == 0 {
                        // println!("break at i==0");
                        break
                    }
                    let c = s.chars().nth(i-1).unwrap();
                    if c.is_whitespace() || c == '(' || c == ')' {
                        // println!("break at c: {}", c);
                        break
                    }
                    i -= 1;
                }
                // println!("i: {}", i);
                i
            };
            let item_str = &s[start..];
            // println!("item_str: {}", item_str);
            s = &s[..start];

            if item_str == "lam" {
                // println!("remainder: {}",s);
                let mut eof = false;
                if let Some(c) = s.chars().last()  {
                    if c != '(' {
                        return Err(format!("ExprSet parse error: `lam` must always have an immediately preceding parenthesis like so `(lam` unless its at the start of the parsed string: {}",s_init))
                    }
                    s = &s[..s.len()-1]; // strip "("
                } else {
                    eof = true;
                };

                let num_items = items_of_depth.pop().ok_or_else(||format!("ExprSet parse error: mismatched parens in: {}",s_init))?;
                if num_items != 1 {
                    return Err(format!("ExprSet parse error: `lam` must always be applied to exactly one argument, like `(lam (foo bar))`: {}",s_init))
                }
                let b: Idx = items.pop().unwrap();
                items.push(self.add(Node::Lam(b)));
                // println!("added lam");
                if eof {
                    if items.len() != 1 {
                        return Err(format!("ExprSet parse error: mismatched parens in: {}",s_init));
                    }
                    return Ok(items.pop().unwrap())
                }
                if let Some(num_items) = items_of_depth.last_mut() {
                    *num_items += 1;
                } else {
                    return Err(format!("ExprSet parse error: mismatched parens in: {}",s_init));
                }
                continue
            }

            let node = {
                if let Some(rest) = item_str.strip_prefix("$") {
                    Node::Var(rest.parse::<i32>().map_err(|e|e.to_string())?)
                } else if let Some(rest) = item_str.strip_prefix("#") {
                    Node::IVar(rest.parse::<i32>().map_err(|e|e.to_string())?)
                } else {
                    Node::Prim(item_str.into())
                }
            };
            items.push(self.add(node));
            *items_of_depth.last_mut().unwrap() += 1;
        }

        if items.len() == 0 {
            return Err("ExprSet parse error: input is empty string".to_string());
        }

        if items_of_depth.len() != 1 {
            return Err(format!("ExprSet parse error: mismatched parens in: {}",s_init));
        }

        let num_items = items_of_depth.pop().unwrap();
        // println!("items outside: {}", num_items);
        for _ in 0..num_items-1 {
            // println!("built an app outside");
            let f: Idx = items.pop().unwrap();
            let x: Idx = items.pop().unwrap();
            items.push(self.add(Node::App(f, x)))
        }
        if items.len() != 1 {
            return Err(format!("ExprSet parse error: mismatched parens in: {}",s_init));
        }

        if self.order == Order::ParentFirst {
            self.nodes[init_len..].reverse();
        }
        Ok((items.pop().unwrap()))
    }
}

impl std::str::FromStr for ExprSet {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // assume uncurried string
        let mut set = ExprSet::empty(Order::ChildFirst, false);
        set.parse_extend(s)?;
        Ok(set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_parse(set: &mut ExprSet, in_s: &str, out_s: &str) {
        let idx = set.parse_extend(in_s).unwrap();
        assert_eq!(set.get(idx).to_string(), out_s.to_string());
    }

    #[test]
    fn test_parse() {
        let set = &mut ExprSet::empty(Order::ChildFirst, false);
        assert_parse(set, "(+ 2 3)", "(+ 2 3)");
        assert_parse(set, "(+ 2 3)", "(+ 2 3)");

        assert_parse(set, "3", "3");
        assert_parse(set, "foo", "foo");

        assert_parse(set, "(foo (bar baz))", "(foo (bar baz))");
        assert_parse(set, "((foo bar) baz)", "(foo bar baz)");

        assert_parse(set, "foo bar baz", "(foo bar baz)");

        assert_parse(set, "(lam b)", "(lam b)");

        assert_parse(set, "lam b", "(lam b)");
        assert_parse(set, "(foo (lam b) (lam c))", "(foo (lam b) (lam c))");
        assert_parse(set, "(lam (+ a b))", "(lam (+ a b))");
        assert_parse(set, "(lam (+ $0 b))", "(lam (+ $0 b))");
        assert_parse(set, "(lam (+ #0 b))", "(lam (+ #0 b))");

        let idx = set.parse_extend("$3").unwrap();
        assert_eq!(set.get(idx).node(), &Node::Var(3));
        let idx = set.parse_extend("#3").unwrap();
        assert_eq!(set.get(idx).node(), &Node::IVar(3));

        assert_parse(set, "(fix_flip $0 (lam (lam (if (is_empty $0) $0 (cons (+ 1 (head $0)) ($1 (tail $0)))))))", "(fix_flip $0 (lam (lam (if (is_empty $0) $0 (cons (+ 1 (head $0)) ($1 (tail $0)))))))")

    }
}