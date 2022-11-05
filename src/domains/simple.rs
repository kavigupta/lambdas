/// This is an example domain, heavily commented to explain how to implement your own!

use crate::*;
use std::collections::HashMap;

/// A simple domain with ints and polymorphic lists (allows nested lists).
/// Generally it's good to be able to imagine the hindley milner type system
/// for your domain so that it's compatible when we add that later. In this case the types
/// would look like `T := (T -> T) | Int | List(T)` where functions are handled
/// by dreamegg::domain::Val so they don't appear here.
#[derive(Clone,Debug, PartialEq, Eq, Hash)]
pub enum SimpleVal {
    Int(i32),
    List(Vec<Val>),
}

#[derive(Clone,Debug, PartialEq, Eq, Hash)]
pub enum SimpleType {
    TInt,
    TList
}

// aliases of various typed specialized to our SimpleVal
type Val = crate::eval::Val<SimpleVal>;
type LazyVal = crate::eval::LazyVal<SimpleVal>;
type Evaluator<'a> = crate::eval::Evaluator<'a,SimpleVal>;
type VResult = crate::eval::VResult<SimpleVal>;
type DSLFn = crate::dsl::DSLFn<SimpleVal>;

// to more concisely refer to the variants
use SimpleVal::*;

use crate::eval::Val::*;
// use domain::Type::*;

// this macro generates two global lazy_static constants: PRIM and FUNCS
// which get used by `val_of_prim` and `fn_of_prim` below. In short they simply
// associate the strings on the left with the rust function and arity on the right.
define_semantics! {
    SimpleVal;
    "+" = (add, "int -> int -> int"),
    "*" = (mul, "int -> int -> int"),
    "map" = (map, "(t0 -> t1) -> (list t0) -> (list t1)"),
    "sum" = (sum, "list int -> int"),
    "0" = "int",
    "1" = "int",
    "2" = "int",
    "[]" = "(list t0)",
    //const "0" = Dom(Int(0)) //todo add support for constants
}


// From<Val> impls are needed for unwrapping values. We can assume the program
// has been type checked so it's okay to panic if the type is wrong. Each val variant
// must map to exactly one unwrapped type (though it doesnt need to be one to one in the
// other direction)
impl FromVal<SimpleVal> for i32 {
    fn from_val(v: Val) -> Result<Self, VError> {
        match v {
            Dom(Int(i)) => Ok(i),
            _ => Err("from_val_to_i32: not an int".into())
        }
    }
}
impl<T: FromVal<SimpleVal>> FromVal<SimpleVal> for Vec<T> {
    fn from_val(v: Val) -> Result<Self, VError> {
        match v {
            Dom(List(v)) => v.into_iter().map(|v| T::from_val(v)).collect(),
            _ => Err("from_val_to_vec: not a list".into())
        }
    }
}

// These Into<Val>s are convenience functions. It's okay if theres not a one to one mapping
// like this in all domains - it just makes .into() save us a lot of work if there is.
impl From<i32> for Val {
    fn from(i: i32) -> Val {
        Dom(Int(i))
    }
}
impl<T: Into<Val>> From<Vec<T>> for Val {
    fn from(vec: Vec<T>) -> Val {
        Dom(List(vec.into_iter().map(|v| v.into()).collect()))
    }
}

// here we actually implement Domain for our domain. 
impl Domain for SimpleVal {
    // we dont use Data here
    type Data = ();
    // type Type = SimpleType;

    // val_of_prim takes a symbol like "+" or "0" and returns the corresponding Val.
    // Note that it can largely just be a call to the global hashmap PRIMS that define_semantics generated
    // however you're also free to do any sort of generic parsing you want, allowing for domains with
    // infinite sets of values or dynamically generated values. For example here we support all integers
    // and all integer lists.
    fn val_of_prim_fallback(p: Symbol) -> Option<Val> {
        // starts with digit -> Int
        if p.as_str().chars().next().unwrap().is_ascii_digit() {
            let i: i32 = p.as_str().parse().ok()?;
            Some(Int(i).into())
        }
        // starts with `[` -> List (must be all ints)
        else if p.as_str().starts_with('[') {
            let intvec: Vec<i32> = serde_json::from_str(p.as_str()).ok()?;
            let valvec: Vec<Val> = intvec.into_iter().map(|v|Dom(Int(v))).collect();
            Some(List(valvec).into())
        } else {
            None
        }
    }

    dsl_entries_lookup_gen!();

    fn type_of_dom_val(&self) -> Type {
        match self {
            Int(_) => Type::base("int".into()),
            List(xs) => {
                let elem_tp = if xs.is_empty() {
                    Type::Var(0) // (list t0)
                } else {
                    // todo here we just use the type of the first entry as the type
                    Self::type_of_dom_val(&xs.first().unwrap().clone().dom().unwrap())
                    // assert!(xs.iter().all(|v| Self::type_of_dom_val(v.clone().dom().unwrap())))
                };
                Type::Term("list".into(),vec![elem_tp])
            },
        }
    }


}


// *** DSL FUNCTIONS ***
// See comments throughout pointing out useful aspects

fn add(mut args: Vec<LazyVal>, handle: &Evaluator) -> VResult {
    // load_args! macro is used to extract the arguments from the args vector. This uses
    // .into() to convert the Val into the appropriate type. For example an int list, which is written
    // as  Dom(List(Vec<Dom(Int)>)), can be .into()'d into a Vec<i32> or a Vec<Val> or a Val.
    load_args!(handle, args, x:i32, y:i32); 
    // ok() is a convenience function that does Ok(v.into()) for you. It relies on your internal primitive types having a one
    // to one mapping to Val variants like `Int <-> i32`. For any domain, the forward mapping `Int -> i32` is guaranteed, however
    // depending on your implementation the reverse mapping `i32 -> Int` may not be. If that's the case you can manually construct
    // the Val from the primitive type like Ok(Dom(Int(v))) for example. Alternatively you can get the .into() property by wrapping
    // all your primitive types eg Int1 = struct(i32) and Int2 = struct(i32) etc for the various types that are i32 under the hood.
    ok(x+y)
}

fn mul(mut args: Vec<LazyVal>, handle: &Evaluator) -> VResult {
    load_args!(handle, args, x:i32, y:i32);
    ok(x*y)
}

fn map(mut args: Vec<LazyVal>, handle: &Evaluator) -> VResult {
    load_args!(handle, args, fn_val: Val, xs: Vec<Val>);
    ok(xs.into_iter()
        // sometimes you might want to apply a value that you know is a function to something else. In that
        // case handle.apply(f: &Val, x: Val) is the way to go. `handle` mainly exists to allow for this, as well
        // as to access handle.data (generic global data) which may be needed for implementation details of certain very complex domains
        // but should largely be avoided.
        .map(|x| handle.apply(&fn_val, x))  
        // here we just turn a Vec<Result> into a Result<Vec> via .collect()'s casting - a handy trick that collapses
        // all the results together into one (which is an Err if any of them was an Err).
        .collect::<Result<Vec<Val>,_>>()?)
}

fn sum(mut args: Vec<LazyVal>, handle: &Evaluator) -> VResult {
    load_args!(handle, args, xs: Vec<i32>);
    ok(xs.iter().sum::<i32>())
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_simple() {
        use domains::simple::SimpleVal;

        fn assert_unify(t1: &str, t2: &str, expected: UnifyResult) {
            let mut ctx = Context::empty();
            let res = ctx.unify(&t1.parse::<Type>().unwrap(),
                        &t2.parse::<Type>().unwrap());
            assert_eq!(res, expected);

            let mut typeset = TypeSet::empty();
            let t1 = typeset.add_tp(&t1.parse::<Type>().unwrap()).instantiate(&mut typeset);
            let t2 = typeset.add_tp(&t2.parse::<Type>().unwrap()).instantiate(&mut typeset);
            let res = typeset.unify(&t1,&t2);
            assert_eq!(res, expected);
        }

        fn assert_infer(p: &str, expected: Result<&str, UnifyErr>) {
            let res = p.parse::<Expr>().unwrap().infer::<SimpleVal>(None, &mut Context::empty(), &mut Default::default());
            assert_eq!(res, expected.map(|ty| ty.parse::<Type>().unwrap()));
        }

        assert_unify("int", "int", Ok(()));
        assert_unify("int", "t0", Ok(()));
        assert_unify("int", "t1", Ok(()));
        assert_unify("(list int)", "(list t1)", Ok(()));
        assert_unify("(int -> bool)", "(int -> t0)", Ok(()));
        assert_unify("t0", "t1", Ok(()));

        assert_infer("3", Ok("int"));
        assert_infer("[1,2,3]", Ok("list int"));
        assert_infer("(+ 2 3)", Ok("int"));
        assert_infer("(lam $0)", Ok("t0 -> t0"));
        assert_infer("(lam (+ $0 1))", Ok("int -> int"));
        assert_infer("map", Ok("((t0 -> t1) -> (list t0) -> (list t1))"));
        assert_infer("(map (lam (+ $0 1)))", Ok("list int -> list int"));

    }

    #[test]
    fn test_eval_simple() {

        assert_execution::<domains::simple::SimpleVal, i32>("(+ 1 2)", &[], 3);

        assert_execution::<domains::simple::SimpleVal, i32>("(sum (map (lam $0) []))", &[], 0);
        

        let arg = SimpleVal::val_of_prim("[1,2,3]".into()).unwrap();
        assert_execution("(map (lam (+ 1 $0)) $0)", &[arg], vec![2,3,4]);

        let arg = SimpleVal::val_of_prim("[1,2,3]".into()).unwrap();
        assert_execution("(sum (map (lam (+ 1 $0)) $0))", &[arg], 9);

        let arg = SimpleVal::val_of_prim("[1,2,3]".into()).unwrap();
        assert_execution("(map (lam (* $0 $0)) (map (lam (+ 1 $0)) $0))", &[arg], vec![4,9,16]);

        let arg = SimpleVal::val_of_prim("[1,2,3]".into()).unwrap();
        assert_execution("(map (lam (* $0 $0)) (map (lam (+ (sum $1) $0)) $0))", &[arg], vec![49,64,81]);

    }
}