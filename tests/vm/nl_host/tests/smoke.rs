use nl::prelude::*;

#[test]
fn opens_and_runs_nl() -> LuaResult<()> {
    let lua = nl::new()?;
    let f = lua.load_nl("pub fn add(a: Number, b: Number) a + b end\nprint \"loaded\"\n", "=smoke")?;
    let exports: Table = f.try_call(())?;
    let add: Function = exports.get("add")?;
    let n: f64 = add.try_call((2, 3))?;
    assert_eq!(n, 5.0);
    assert_eq!(lua.type_of(&Value::Number(1.5))?, "Number");
    Ok(())
}

/// run.sh builds the crate twice; NL_HOST_JIT=off is the build of the VM
/// with -DLUAJIT_DISABLE_JIT.
#[test]
fn the_jit_is_as_the_build_says() -> LuaResult<()> {
    let lua = nl::new()?;
    let on: bool = lua.load("return (jit.status())").eval()?;
    assert_eq!(on, std::env::var("NL_HOST_JIT").as_deref() != Ok("off"));
    Ok(())
}
