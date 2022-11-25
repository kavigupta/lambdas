use std::{collections::VecDeque};
use crate::parse_type;
use crate::*;
use crate::dsl::Domain;
use once_cell::sync::Lazy;
use serde::{Serialize, Deserialize};


// use std::{path::Path, collections::HashMap, fs::File};
// use serde_json::from_reader;

// /// Mapping from DSL symbols to types
// pub struct TypeMap {
//     pub map: HashMap<Symbol, Type>,
// }

// pub fn load_types(path: &Path) -> TypeMap {
//     let res: HashMap<Symbol,String> = from_reader(File::open(path).expect("file not found")).expect("json deserializing error");
//     TypeMap { map: res.into_iter().map(|(k,v)| (k,v.parse::<Type>().unwrap())).collect() }
// }


#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnifyErr {
    Occurs,
    ConcreteSubtree,
    Production
}
pub type UnifyResult = Result<(), UnifyErr>;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Type {
    Var(usize), // type variable like t0 t1 etc
    Term(Symbol, Vec<Type>), // symbol is the name like "int" or "list" or "->" and Vec<Type> is the args which is empty list for things like int etc
    // Arrow(Box<Type>,Box<Type>)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TNode {
    Var(usize), // type variable like t0 t1 etc
    Term(Symbol, Vec<RawTypeRef>), // symbol is the name like "int" or "list" or "->" and Vec is the args
    // Arrow(RawTypeRef,RawTypeRef)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeRef {
    pub raw: RawTypeRef,
    pub shift: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RawTypeRef(usize);


impl RawTypeRef {
    pub fn shift(&self, shift: usize) -> TypeRef {
        TypeRef::new(*self,shift)
    }

    pub fn node<'a>(&self, typeset: &'a TypeSet) -> &'a TNode {
        &typeset.nodes[self.0]
    }

    /// convenience method for converting to types. probably super slow but useful for debugging
    #[inline(never)]
    pub fn tp(&self, typeset: &TypeSet) -> Type {
        match self.node(typeset) {
            TNode::Var(i) => Type::Var(*i),
            TNode::Term(p, args) => {
                Type::Term(p.clone(), args.iter().map(|arg| arg.tp(typeset)).collect())
            },
        }
    }

    pub fn show(&self, typeset: &TypeSet) -> String {
        self.tp(typeset).to_string()
    }

    pub fn as_arrow(&self, typeset: &TypeSet) -> Option<(RawTypeRef, RawTypeRef)> {
        if let TNode::Term(name,args) = self.node(typeset) {
            if *name != *ARROW_SYM {
                return None
            }
            let mut it = args.iter();
            let left = it.next().unwrap();
            let right = it.next().unwrap();
            assert!(it.next().is_none(), "malformed arrow");
            Some((*left,*right))
        } else {
             None
        }
    }

    pub fn is_arrow(&self, typeset: &TypeSet) -> bool {
        if let TNode::Term(name,_) = self.node(typeset) {
            return *name == *ARROW_SYM
        }
        false
    }

    /// iterates over all nodes in the term of this type
    pub fn iter_arrows<'a>(&self, typeset: &'a TypeSet) -> ArrowIterTypeRef<'a> {
        ArrowIterTypeRef { curr: *self, typeset }
    }

    /// iterates over uncurried argument types of this arrow type
    pub fn iter_args<'a>(&self, typeset: &'a TypeSet) -> impl Iterator<Item=RawTypeRef> + 'a {
        self.iter_arrows(typeset).map(|(left,_right)| left)
    }

    /// arity of this arrow type (zero if not an arrow type)
    pub fn arity(&self, typeset: &TypeSet) -> usize {
        self.iter_args(typeset).count()
    }

    /// return type of this arrow types *after* uncurrying. For a non arrow type
    /// this just returns the type itself.
    pub fn return_type(&self, typeset: &TypeSet) -> RawTypeRef {
        self.iter_arrows(typeset).last().map(|(_left,right)| right).unwrap_or(*self)
    }

    /// true if there are no type vars in this type
    pub fn is_concrete(&self, typeset: &TypeSet) -> bool {
        match self.node(typeset) {
            TNode::Var(_) => false,
            TNode::Term(_,args) => args.iter().all(|ty| ty.is_concrete(typeset)),
        }
    }

    pub fn max_var(&self, typeset: &TypeSet) -> Option<usize> {
        typeset.max_vars[self.0]
    }

    pub fn instantiate(&self, typeset: &mut TypeSet) -> TypeRef {
        let shift_by = typeset.next_var;
        if let Some(max_var) = self.max_var(typeset) {
            // create a fresh type var for each new variable
            for _ in 0..=max_var {
                typeset.fresh_type_var();
            }
        }
        TypeRef::new(*self, shift_by)
    }

}


impl TypeRef {
    fn new(raw: RawTypeRef, shift: usize) -> TypeRef {
        TypeRef {raw, shift}
    }

    /// if `self` is a Var that is bound by our context, return whatever it is bound to 
    pub fn canonicalize(&self, typeset: &TypeSet) -> TypeRef {
        if let TNode::Var(i) = self.raw.node(typeset) {
            if let Some(tp_ref) = typeset.get_var(*i + self.shift) {
                // println!("looked up t{} -> {}", *i + self.shift, tp_ref.show(typeset));
                return tp_ref.canonicalize(typeset) // recursively resolve the lookup result
            }
        }
        *self
    }

    pub fn tp(&self, typeset: &TypeSet) -> Type {
        self.raw.tp(typeset)
    }

    pub fn show(&self, typeset: &TypeSet) -> String {
        format!("[shift {}] {}", self.shift, self.raw.tp(typeset))
    }

    pub fn as_arrow(&self, typeset: &TypeSet) -> Option<(TypeRef, TypeRef)> {
        let canonical = self.canonicalize(typeset);
        canonical.raw.as_arrow(typeset).map(|(r1,r2)| (r1.shift(canonical.shift),r2.shift(canonical.shift)))
    }

    pub fn is_arrow(&self, typeset: &TypeSet) -> bool {
        if let TNode::Term(name,_) = self.canonicalize(typeset).raw.node(typeset) {
            return *name == *ARROW_SYM
        }
        false
    }

    /// iterates over all nodes in the term of this type
    pub fn iter_arrows<'a>(&'a self, typeset: &'a TypeSet) -> impl Iterator<Item=(TypeRef,TypeRef)> + 'a {
        let canonical = self.canonicalize(typeset);
        canonical.raw.iter_arrows(typeset).map(move |(r1,r2)| (r1.shift(canonical.shift),r2.shift(canonical.shift)))
    }

    /// iterates over uncurried argument types of this arrow type
    pub fn iter_args<'a>(&'a self, typeset: &'a TypeSet) -> impl Iterator<Item=TypeRef> + 'a {
        self.iter_arrows(typeset).map(|(left,_right)| left)
    }

    /// arity of this arrow type (zero if not an arrow type)
    pub fn arity(&self, typeset: &TypeSet) -> usize {
        self.iter_args(typeset).count()
    }

    /// return type of this arrow types *after* uncurrying. For a non arrow type
    /// this just returns the type itself.
    pub fn return_type(&self, typeset: &TypeSet) -> TypeRef {
        self.iter_arrows(typeset).last().map(|(_left,right)| right).unwrap_or(*self)
    }

    /// true if there are no type vars in this type
    pub fn is_concrete(&self, typeset: &TypeSet) -> bool {
        let canonical = self.canonicalize(typeset);
        match canonical.raw.node(typeset) {
            TNode::Var(_) => false,
            TNode::Term(_, args) => args.iter().map(|r|r.shift(canonical.shift)).all(|ty| ty.is_concrete(typeset)),
        }
    }

    /// true if type var i occurs in this type (post-shifting of this type)
    pub fn occurs(&self, i: usize, typeset: &TypeSet) -> bool {
        // println!("occccc");
        // todo!() // not sure if need to run substitution here
        // println!("{:?}", self);
        // println!("before canonicalizing: {}", self.show(typeset));
        // println!("canonical: {}", self.canonicalize(typeset).show(typeset));
        // println!("{:?}", self.resolve(typeset));

        // let resolved = self.resolve(typeset);
        // let shift = self.canonicalize(typeset).shift;
        let canonical = self.canonicalize(typeset);

        // println!("resolved: {:?}", resolved);

        match canonical.raw.node(typeset) {
            TNode::Var(j)  => i == j + canonical.shift,
            TNode::Term(_, args) => {
                // println!("args: {:?}", self.iter_term_args(typeset).map(|arg|arg.show(typeset)).collect::<Vec<_>>());
                args.iter().map(|raw| raw.shift(canonical.shift)).any(|ty| ty.occurs(i, typeset))
            },
        }
    }

}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeSet {
    pub nodes: Vec<TNode>,
    pub max_vars: Vec<Option<usize>>,
    pub subst: Vec<(usize,TypeRef)>,
    pub next_var: usize,
}

impl TypeSet {
    pub fn add_tp(&mut self, tp: &Type) -> RawTypeRef {
        match tp {
            Type::Var(i) => {
                self.add_node(TNode::Var(*i))
            }
            Type::Term(p, args) => {
                let args = args.iter().map(|arg| self.add_tp(arg)).collect();
                self.add_node(TNode::Term(p.clone(), args))
            },
        }
    }
    #[inline(always)]
    pub fn add_node(&mut self, node: TNode) -> RawTypeRef {
        let max_var = match &node {
            TNode::Var(i) => Some(*i),
            TNode::Term(_, args) => args.iter().filter_map(|raw| raw.max_var(self)).max(),
        };
        self.max_vars.push(max_var);
        self.nodes.push(node);
        RawTypeRef(self.nodes.len() - 1)
    }
    // #[inline(always)]
    // pub fn add_arrow(&mut self, left: RawTypeRef, right: RawTypeRef) -> RawTypeRef {
    //     let arg2 = self.add_node(TNode::ArgCons(right, None));
    //     let args = self.add_node(TNode::ArgCons(left, Some(arg2)));
    //     self.add_node(TNode::Term(ARROW_SYM.clone(), Some(args)))
    // }
    /// This is the usual way of creating a new Context. The context will be append-only
    /// meaning you can roll it back to a point by truncating
    pub fn empty() -> TypeSet {
        TypeSet {
            nodes: Default::default(),
            max_vars: Default::default(),
            subst: Default::default(),
            next_var: 0,
        }
    }

    pub fn save_state(&self) -> (usize,usize) {
        (self.subst.len(), self.next_var)
    }

    pub fn load_state(&mut self, state: (usize,usize)) {
        self.subst.truncate(state.0);
        self.next_var = state.1;
    }

    fn fresh_type_var(&mut self) -> Type {
        self.next_var += 1;
        Type::Var(self.next_var-1)
    }

    // /// adds new fresh type vars as necessary such that variable Var exists
    // #[inline(always)]
    // fn fresh_type_vars(&mut self, var: usize) {
    //     while var >= self.next_var {
    //         self.fresh_type_var();
    //     }
    // }

    /// a very quick non-allocating check that returns false if it's
    /// obvious that these types won't unify. This works *even when a type hasnt
    /// been instantiated() to have new type variables*. First this checks if t1 and t2 have the same constructors
    /// and if theres an obvious mismatch there it gives up. Then it goes and looks up the types in the ctx
    /// in case they were typevars, and then again checks if they have th same constructor. It uses apply_immut() to
    /// avoid mutating the context for this lookup.
    /// Note the apply_immut version of this was wrong bc thats only safe to do on the hole_tp side and apply_immut
    /// is already done to the hole before then anyways
    pub fn might_unify(&self, t1: &RawTypeRef, t2: &TypeRef) -> bool {
        let node1 = t1.node(self);
        let canonical2 = t2.canonicalize(self);
        let node2 = canonical2.raw.node(self);
        // let node2 = t2.resolve(self);
        // let shift = t2.canonicalize(self).shift;
        match (node1,node2) {
            (TNode::Var(_), TNode::Var(_)) => true,
            (TNode::Var(_), TNode::Term(_, _)) => true,
            (TNode::Term(_, _), TNode::Var(_)) => true,
            (TNode::Term(x, xs), TNode::Term(y, ys)) => {
                *x == *y && xs.len() == ys.len() && xs.iter().zip(ys.iter().map(|raw|raw.shift(canonical2.shift))).all(|(x,y)| self.might_unify(x,&y))
            },
        }
    }

    /// Normal unification. Does not do the amortizing step of the unionfind (but may mutate
    /// it still). See unify_cached() for amortized unionfind. Note that this is likely not slower
    /// than unify_cached() in most cases.
    pub fn unify(&mut self, t1: &TypeRef,  t2: &TypeRef) -> UnifyResult {
        // println!("\tunify({},{})", t1.show(self), t2.show(self));
        // println!("\t->({:?},{:?})", t1.resolve(self), t2.resolve(self));
        // let t1: Type = t1.apply(self);
        // let t2: Type = t2.apply(self);
        // println!("\t  ...({},{}) {}", t1, t2, self);
        // println!("about to resolve");

        let canonical1 = t1.canonicalize(self);
        let canonical2 = t2.canonicalize(self);
        let node1 = canonical1.raw.node(self);
        let node2 = canonical2.raw.node(self);

        match (node1,node2) {
            (TNode::Var(i), _) => {
                let i_shifted = i + canonical1.shift;
                // check for identical variable (only needs to happen on this match case bc later one cant have a Var for both)
                if let TNode::Var(j) = node2 {
                    if i_shifted == j + canonical2.shift {
                        return Ok(()); // unify(t0, t0) -> true
                    }
                }
                // *** "occurs" check, which prevents recursive definitions of types. Removing it would allow them.
                if canonical2.occurs(i_shifted, self) { return Err(UnifyErr::Occurs) } // recursive type  e.g. unify(t0, (t0 -> int)) -> false

                // set the varisble
                assert!(self.get_var(i_shifted).is_none());
                self.set_var(i_shifted, canonical2);
                Ok(())
            }
            (_, TNode::Var(i)) => {
                let i_shifted = i + canonical2.shift;
                // *** "occurs" check, which prevents recursive definitions of types. Removing it would allow them.
                if canonical1.occurs(i_shifted, self) { return Err(UnifyErr::Occurs) } // recursive type  e.g. unify(t0, (t0 -> int)) -> false

                // set the varisble
                assert!(self.get_var(i_shifted).is_none());
                self.set_var(i_shifted, canonical1);
                Ok(())
            }

            (TNode::Term(x, xs), TNode::Term(y, ys)) =>
            {
                // println!("resolved");
                // simply recurse
                if x != y || xs.len() != ys.len() {
                    return Err(UnifyErr::Production)
                }
                // todo ugh lame collect()
                xs.iter().map(|r|r.shift(canonical1.shift))
                    .zip(ys.iter().map(|r|r.shift(canonical2.shift)))
                    .collect::<Vec<_>>().into_iter()
                    .try_for_each(|(x,y)| self.unify(&x,&y))
            }
        }
    }

    /// get what a variable is bound to (if anything).
    // #[inline(always)]
    fn get_var(&self, var: usize) -> Option<&TypeRef> {
        self.subst.iter().rfind(|(i,_)| *i == var).map(|(_,tp)| tp)
    }
    /// set what a variable is bound to
    #[inline(always)]
    fn set_var(&mut self, var: usize, ty: TypeRef) {
        self.subst.push((var,ty));
    }
}



pub static ARROW_SYM: Lazy<Symbol> = Lazy::new(|| Symbol::from("->"));

impl Type {
    pub fn base(name: Symbol) -> Type {
        Type::Term(name, vec![])
    }

    pub fn arrow(left: Type, right: Type) -> Type {
        Type::Term(ARROW_SYM.clone(), vec![left, right])
    }

    pub fn is_arrow(&self) -> bool {
        match self {
            Type::Var(_) => false,
            Type::Term(name, _) => *name == *ARROW_SYM,
        }
    }

    pub fn as_arrow(&self) -> Option<(&Type, &Type)> {
        match self {
            Type::Term(name,args) => {
                if *name != *ARROW_SYM {
                    return None
                }
                assert_eq!(args.len(),2);
                Some((&args[0], &args[1]))
            },
            _ => None
        }
    }

    /// iterates over all (left_type,right_type) pairs for the chain of arrows
    /// starting here. Empty iterator if this is not an arrow.
    // pub fn iter_nodes(&self) -> impl Iterator<Item=&Type> {
    //     return NodeIter { curr: self }
    // }

    /// iterates over all nodes in the term of this type
    pub fn iter_arrows(&self) -> ArrowIter {
        ArrowIter { curr: self }
    }

    /// iterates over uncurried argument types of this arrow type
    pub fn iter_args(&self) -> impl Iterator<Item=&Type> {
        self.iter_arrows().map(|(left,_right)| left)
    }

    /// arity of this arrow type (zero if not an arrow type)
    pub fn arity(&self) -> usize {
        self.iter_args().count()
    }

    /// return type of this arrow types *after* uncurrying. For a non arrow type
    /// this just returns the type itself.
    pub fn return_type(&self) -> &Type {
        self.iter_arrows().last().map(|(_left,right)| right).unwrap_or(self)
    }

    /// true if there are no type vars in this type
    pub fn is_concrete(&self) -> bool {
        match self {
            Type::Var(_) => false,
            Type::Term(_, args) => args.iter().all(|ty| ty.is_concrete())
        }
    }

    /// true if type var i occurs in this type
    pub fn occurs(&self, i: usize) -> bool {
        match self {
            Type::Var(j)  => i == *j,
            Type::Term(_, args) => args.iter().any(|ty| ty.occurs(i))
        }
    }

    pub fn apply_cached(&self, ctx: &mut Context) -> Type {
        if self.is_concrete() {
            return self.clone();
        }
        match self {
            Type::Var(i) => {
                // look up the type var in the ctx to see if its bound
                if let Some(tp) = ctx.get(*i).cloned() {
                    // in case it's bound to something that ALSO has variables, we want to track those down too
                    let tp_applied = tp.apply(ctx);
                    if tp != tp_applied {
                        // and to save our work for the future, lets amortize it (union-find style) by saving what we
                        // found things were bound to. Since bindings will never change this is okay.
                        ctx.set(*i, tp_applied.clone())
                    }
                    tp_applied
                } else {
                    self.clone() // t0 is not bound by ctx so we leave it unbound
                }
            },
            Type::Term(name, args) => Type::Term(name.clone(), args.iter().map(|ty| ty.apply_cached(ctx)).collect())
        }
    }

    /// same as apply_cached() but doesnt do the unionfind style caching of results, so there's no need to mutate the ctx
    pub fn apply(&self, ctx: &Context) -> Type {
        if self.is_concrete() {
            return self.clone();
        }
        match self {
            Type::Var(i) => {
                // look up the type var in the ctx to see if its bound
                if let Some(tp) = ctx.get(*i).cloned() {
                    // in case it's bound to something that ALSO has variables, we want to track those down too
                    tp.apply(ctx)
                } else {
                    self.clone() // t0 is not bound by ctx so we leave it unbound
                }
            },
            Type::Term(name, args) => Type::Term(name.clone(), args.iter().map(|ty| ty.apply(ctx)).collect())
        }
    }


    /// shifts all variables in a type such that they are fresh variables in the context, returning a new type
    pub fn instantiate(&self, ctx: &mut Context) -> Type {
        if self.is_concrete() {
            return self.clone()
        }
        fn instantiate_aux(ty: &Type, ctx: &mut Context, shift_by: usize) -> Type {
            match ty {
                Type::Var(i) => {
                    let new = i + shift_by;
                    ctx.fresh_type_vars(new);
                    assert!(ctx.get(new).is_none());
                    Type::Var(new)
                },
                Type::Term(name, args) => Type::Term(name.clone(), args.iter().map(|t| instantiate_aux(t, ctx, shift_by)).collect()),
            }
        }
        // shift by the highest var that already exists, so that theres no conflict
        instantiate_aux(self, ctx, ctx.next_var)
    }
}

pub struct ArrowIter<'a> {
    curr: &'a Type
}

impl<'a> Iterator for ArrowIter<'a> {
    type Item = (&'a Type, &'a Type);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((left,right)) = self.curr.as_arrow() {
            self.curr = right;
            Some((left,right))
        } else {
            None
        }
    }
}

pub struct ArrowIterTypeRef<'a> {
    typeset: &'a TypeSet,
    curr: RawTypeRef,
}

impl<'a> Iterator for ArrowIterTypeRef<'a> {
    type Item = (RawTypeRef,RawTypeRef);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((left,right)) = self.curr.as_arrow(self.typeset) {
            self.curr = right;
            Some((left,right))
        } else {
            None
        }
    }
}



impl std::str::FromStr for Type {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_type::parse(s)
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn helper(ty: &Type, f: &mut std::fmt::Formatter<'_>, arrow_parens: bool) -> std::fmt::Result {
            match ty {
                Type::Var(i) => write!(f,"t{}", i),
                Type::Term(name, args) => {
                    if args.is_empty() {
                        write!(f, "{}", name)
                    } else if *name == *ARROW_SYM {
                        assert_eq!(args.len(), 2);
                        // write!(f, "({} {} {})", &args[0], name, &args[1])
                        if arrow_parens {
                            write!(f, "(")?;
                        }
                        helper(&args[0], f, true)?;
                        write!(f, " {} ", ARROW_SYM.as_ref())?;
                        helper(&args[1], f, false)?;
                        if arrow_parens {
                            write!(f, ")")?;
                        }
                        Ok(())
                    } else {
                        write!(f, "({}", name)?;
                        for arg in args.iter() {
                            write!(f, " ")?;
                            helper(arg, f, true)?;
                        }
                        write!(f, ")")
                    }
                },
            }
        }
        helper(self, f, true)
    }
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    subst_unionfind: Vec<Option<Type>>, // todo also try ahashmap tho i just wanted to avoid the allocations
    subst_append_only: Vec<(usize,Type)>,
    next_var: usize,
    append_only: bool,
}

impl Context {

    /// This is the usual way of creating a new Context. The context will be append-only
    /// meaning you can roll it back to a point by truncating
    pub fn empty() -> Context {
        Context {
            subst_unionfind: Default::default(),
            subst_append_only: Default::default(),
            next_var: 0,
            append_only: true,
        }
    }

    /// instead of an append-only substitution, the context will instead use a unionfind. This is honestly
    /// likely not noticably faster and doesnt allow rollbacks. It may even be slower.
    pub fn empty_unionfind() -> Context {
        Context {
            subst_unionfind: Default::default(),
            subst_append_only: Default::default(),
            next_var: 0,
            append_only: false,
        }
    }

    pub fn save_state(&self) -> (usize,usize) {
        assert!(self.append_only);
        (self.subst_append_only.len(), self.next_var)
    }

    pub fn load_state(&mut self, state: (usize,usize)) {
        assert!(self.append_only);
        self.subst_append_only.truncate(state.0);
        self.next_var = state.1;
    }

    fn fresh_type_var(&mut self) -> Type {
        if !self.append_only {
            self.subst_unionfind.push(None);
        }
        self.next_var += 1;
        Type::Var(self.next_var-1)
    }

    /// adds new fresh type vars as necessary such that variable Var exists
    #[inline(always)]
    fn fresh_type_vars(&mut self, var: usize) {
        while var >= self.next_var {
            self.fresh_type_var();
        }
    }

    /// a very quick non-allocating check that returns false if it's
    /// obvious that these types won't unify. This works *even when a type hasnt
    /// been instantiated() to have new type variables*. First this checks if t1 and t2 have the same constructors
    /// and if theres an obvious mismatch there it gives up. Then it goes and looks up the types in the ctx
    /// in case they were typevars, and then again checks if they have th same constructor. It uses apply_immut() to
    /// avoid mutating the context for this lookup.
    /// Note the apply_immut version of this was wrong bc thats only safe to do on the hole_tp side and apply_immut
    /// is already done to the hole before then anyways
    pub fn might_unify(t1: &Type, t2: &Type) -> bool {
        match (t1,t2) {
            (Type::Var(_), Type::Var(_)) => true,
            (Type::Var(_), Type::Term(_, _)) => true,
            (Type::Term(_, _), Type::Var(_)) => true,
            (Type::Term(x, xs), Type::Term(y, ys)) => {
                x == y && xs.len() == ys.len() && xs.iter().zip(ys.iter()).all(|(x,y)| Context::might_unify(x,y))
            },
        }
    }

    /// Normal unification. Does not do the amortizing step of the unionfind (but may mutate
    /// it still). See unify_cached() for amortized unionfind. Note that this is likely not slower
    /// than unify_cached() in most cases.
    pub fn unify(&mut self, t1: &Type,  t2: &Type) -> UnifyResult {
        // println!("\tunify({},{}) {}", t1, t2, self);
        let t1: Type = t1.apply(self);
        let t2: Type = t2.apply(self);
        // println!("\t  ...({},{}) {}", t1, t2, self);
        if t1.is_concrete() && t2.is_concrete() {
            // if both types are concrete, simple equality works because we dont need to do any fancy variable binding
            if t1 == t2 {
                return Ok(())
            } else {
                return Err(UnifyErr::ConcreteSubtree)
            }
        }
        match (t1, t2) {
            (Type::Var(i), ty) | (ty, Type::Var(i)) => {
                if ty == Type::Var(i) { return Ok(()) } // unify(t0, t0) -> true
                if ty.occurs(i) { return Err(UnifyErr::Occurs) } // recursive type  e.g. unify(t0, (t0 -> int)) -> false
                // *** Above is the "occurs" check, which prevents recursive definitions of types. Removing it would allow them.

                assert!(self.get(i).is_none());
                self.set(i, ty);
                Ok(())
            },
            (Type::Term(x, xs), Type::Term(y, ys)) => {
                // simply recurse
                if x != y || xs.len() != ys.len() {
                    return Err(UnifyErr::Production)
                }
                xs.iter().zip(ys.iter()).try_for_each(|(x,y)| self.unify(x,y))
            }
        }
    }

    /// [expert mode] like unify() but uses apply_cached() to do amortization step of
    /// unionfind. Likely not worth using compared to unify().
    pub fn unify_cached(&mut self, t1: &Type,  t2: &Type) -> UnifyResult {
        // println!("unify({},{}) {}", t1, t2, self);
        let t1: Type = t1.apply_cached(self);
        let t2: Type = t2.apply_cached(self);
        // println!("  ...({},{}) {}", t1, t2, self);
        if t1.is_concrete() && t2.is_concrete() {
            // if both types are concrete, simple equality works because we dont need to do any fancy variable binding
            if t1 == t2 {
                return Ok(())
            } else {
                return Err(UnifyErr::ConcreteSubtree)
            }
        }
        match (t1, t2) {
            (Type::Var(i), ty) | (ty, Type::Var(i)) => {
                if ty == Type::Var(i) { return Ok(()) } // unify(t0, t0) -> true
                if ty.occurs(i) { return Err(UnifyErr::Occurs) } // recursive type  e.g. unify(t0, (t0 -> int)) -> false
                // *** Above is the "occurs" check, which prevents recursive definitions of types. Removing it would allow them.

                assert!(self.subst_unionfind.get(i).is_none());
                self.set(i, ty);
                Ok(())
            },
            (Type::Term(x, xs), Type::Term(y, ys)) => {
                // simply recurse
                if x != y || xs.len() != ys.len() {
                    return Err(UnifyErr::Production)
                }
                xs.iter().zip(ys.iter()).try_for_each(|(x,y)| self.unify(x,y))
            }
        }
    }

    /// get what a variable is bound to (if anything).
    #[inline(always)]
    fn get(&self, var: usize) -> Option<&Type> { // todo written in a silly way, rewrite
        if self.append_only {
            self.subst_append_only.iter().rfind(|(i,_)| *i == var).map(|(_,tp)| tp)
        } else {
            self.subst_unionfind[var].as_ref()
        }
    }
    /// set what a variable is bound to
    #[inline(always)]
    fn set(&mut self, var: usize, ty: Type) {
        if self.append_only {
            self.subst_append_only.push((var,ty));
        } else {
            self.subst_unionfind[var] = Some(ty);
        }
    }

}

impl std::fmt::Display for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,"{{")?;
        let mut first: bool = true;
        for (i, item) in self.subst_unionfind.iter().enumerate() {
            if let Some(ty) = item {
                if !first { write!(f, ", ")? } else { first = false }
                write!(f, "{}:{}", i, ty)?
            }
        }
        write!(f,"}}")
    }
}


impl<'a> Expr<'a> {
    pub fn infer<D: Domain>(&self, ctx: &mut Context, env: &mut VecDeque<Type>, dsl: &DSL<D>) -> Result<Type,UnifyErr> {
        // println!("infer({})", self.to_string_uncurried(child));
        match self.node() {
            Node::App(f,x) => {
                let return_tp = ctx.fresh_type_var();
                let x_tp = self.get(*x).infer::<D>(ctx, env, dsl)?;
                let f_tp = self.get(*f).infer::<D>(ctx, env, dsl)?;
                ctx.unify(&f_tp, &Type::arrow(x_tp, return_tp.clone()))?;
                Ok(return_tp.apply(ctx))
            },
            Node::Lam(b) => {
                let var_tp = ctx.fresh_type_var();
                // todo maybe optimize by making this a vecdeque for faster insert/remove at the zero index
                env.push_front(var_tp.clone());
                let body_tp = self.get(*b).infer::<D>(ctx, env, dsl)?;
                env.pop_front();
                Ok(Type::arrow(var_tp, body_tp).apply(ctx))
            },
            Node::Var(i) => {
                if (*i as usize) >= env.len() {
                    panic!("unbound variable encountered during infer(): ${}", i)
                }
                Ok(env[*i as usize].apply(ctx))
            },
            Node::IVar(_i) => {
                // interesting, I guess we can have this and it'd probably be easy to do
                unimplemented!();
            }
            Node::Prim(p) => {
                Ok(dsl.type_of_prim(p).instantiate(ctx))
            },
        }
    }
    // pub fn infer_ref<D: Domain>(&self, ctx: &mut TypeSet, env: &mut VecDeque<TypeRef>) -> Result<TypeRef,UnifyErr> {
    //     // println!("infer({})", self.to_string_uncurried(child));
    //     match self.node() {
    //         Node::App(f,x) => {
    //             let return_tp = ctx.fresh_type_var();
    //             let x_tp = self.get(*x).infer_ref::<D>(ctx, env)?;
    //             let f_tp = self.get(*f).infer_ref::<D>(ctx, env)?;
    //             let arrow_tp = ctx.add_arrow(f_tp, x_tp);
    //             ctx.unify(&f_tp, &Type::arrow(x_tp, return_tp.clone()))?;
    //             Ok(return_tp.apply(ctx))
    //         },
    //         Node::Lam(b) => {
    //             let var_tp = ctx.fresh_type_var();
    //             // todo maybe optimize by making this a vecdeque for faster insert/remove at the zero index
    //             env.push_front(var_tp.clone());
    //             let body_tp = self.get(*b).infer_ref::<D>(ctx, env)?;
    //             env.pop_front();
    //             Ok(Type::arrow(var_tp, body_tp).apply(ctx))
    //         },
    //         Node::Var(i) => {
    //             if (*i as usize) >= env.len() {
    //                 panic!("unbound variable encountered during infer(): ${}", i)
    //             }
    //             Ok(env[*i as usize].apply(ctx))
    //         },
    //         Node::IVar(_i) => {
    //             // interesting, I guess we can have this and it'd probably be easy to do
    //             unimplemented!();
    //         }
    //         Node::Prim(p) => {
    //             Ok(D::type_of_prim(p).instantiate(ctx))
    //         },
    //     }
    // }
    
}


