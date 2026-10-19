# `>>`, `>>>`, `>=`, `>>=` closing type arguments are split.
let a: Box<Box<T>> = x
let b: A<B<C<D>>> = x
let c: Box<T>= x
let d: Box<Box<T>>= x
