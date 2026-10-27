use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use gc_arena::Collect;
use piccolo::{
    BoxSequence, Callback, CallbackReturn, Closure, Context, Error, Execution, Executor, Fuel, Lua,
    Sequence, SequencePoll, Stack,
};

const BASE_STEP_FUEL: i32 = 4;
const CALLBACK_FUEL: i32 = 8;
const SEQUENCE_FUEL: i32 = 4;

#[test]
fn checked_fuel_never_creates_debt() {
    let mut fuel = Fuel::with(3);
    assert!(!fuel.try_consume(4));
    assert_eq!(fuel.remaining(), 3);
    fuel.consume(4);
    assert_eq!(fuel.remaining(), 0);
    fuel.adjust(-1);
    assert_eq!(fuel.remaining(), 0);
    fuel.refill(-1, -1);
    assert_eq!(fuel.remaining(), 0);
}

#[test]
fn callback_requires_complete_fixed_cost_before_visible_side_effect() {
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = Arc::clone(&calls);
    let mut lua = Lua::empty();
    let executor = lua.enter(|ctx| {
        let callback = Callback::from_fn(&ctx, move |_, _, _| {
            callback_calls.fetch_add(1, Ordering::SeqCst);
            Ok(CallbackReturn::Return)
        });
        ctx.stash(Executor::start(ctx, callback.into(), ()))
    });

    let mut short = Fuel::with(BASE_STEP_FUEL + CALLBACK_FUEL - 1);
    assert!(!lua.enter(|ctx| ctx.fetch(&executor).step(ctx, &mut short)));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(short.remaining(), CALLBACK_FUEL - 1);

    let mut exact = Fuel::with(BASE_STEP_FUEL + CALLBACK_FUEL);
    assert!(lua.enter(|ctx| ctx.fetch(&executor).step(ctx, &mut exact)));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(exact.remaining(), 0);
}

static SEQUENCE_POLLS: AtomicUsize = AtomicUsize::new(0);

#[derive(Collect)]
#[collect(require_static)]
struct ObservableSequence;

impl<'gc> Sequence<'gc> for ObservableSequence {
    fn poll(
        &mut self,
        _ctx: Context<'gc>,
        _exec: Execution<'gc, '_>,
        _stack: Stack<'gc, '_>,
    ) -> Result<SequencePoll<'gc>, Error<'gc>> {
        SEQUENCE_POLLS.fetch_add(1, Ordering::SeqCst);
        Ok(SequencePoll::Return)
    }
}

#[test]
fn sequence_requires_complete_fixed_cost_before_polling() {
    SEQUENCE_POLLS.store(0, Ordering::SeqCst);
    let mut lua = Lua::empty();
    let executor = lua.enter(|ctx| {
        let callback = Callback::from_fn(&ctx, |ctx, _, _| {
            Ok(CallbackReturn::Sequence(BoxSequence::new(
                &ctx,
                ObservableSequence,
            )))
        });
        ctx.stash(Executor::start(ctx, callback.into(), ()))
    });

    let mut callback_fuel = Fuel::with(BASE_STEP_FUEL + CALLBACK_FUEL);
    assert!(!lua.enter(|ctx| ctx.fetch(&executor).step(ctx, &mut callback_fuel)));
    assert_eq!(SEQUENCE_POLLS.load(Ordering::SeqCst), 0);

    let mut short = Fuel::with(BASE_STEP_FUEL + SEQUENCE_FUEL - 1);
    assert!(!lua.enter(|ctx| ctx.fetch(&executor).step(ctx, &mut short)));
    assert_eq!(SEQUENCE_POLLS.load(Ordering::SeqCst), 0);
    assert_eq!(short.remaining(), SEQUENCE_FUEL - 1);

    let mut exact = Fuel::with(BASE_STEP_FUEL + SEQUENCE_FUEL);
    assert!(lua.enter(|ctx| ctx.fetch(&executor).step(ctx, &mut exact)));
    assert_eq!(SEQUENCE_POLLS.load(Ordering::SeqCst), 1);
    assert_eq!(exact.remaining(), 0);
}

#[test]
fn callback_dense_script_cannot_execute_through_negative_fuel() {
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = Arc::clone(&calls);
    let mut lua = Lua::empty();
    let executor = lua
        .try_enter(|ctx| {
            ctx.set_global(
                "side_effect",
                Callback::from_fn(&ctx, move |_, _, _| {
                    callback_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(CallbackReturn::Return)
                }),
            )?;
            let closure = Closure::load(
                ctx,
                Some("callback-dense.lua"),
                b"for i = 1, 1000 do side_effect() end".as_slice(),
            )?;
            Ok(ctx.stash(Executor::start(ctx, closure.into(), ())))
        })
        .unwrap();

    let budget = 200;
    let mut fuel = Fuel::with(budget);
    assert!(!lua.enter(|ctx| ctx.fetch(&executor).step(ctx, &mut fuel)));
    assert!(fuel.remaining() >= 0);
    assert!(calls.load(Ordering::SeqCst) <= (budget / (BASE_STEP_FUEL + CALLBACK_FUEL)) as usize);
}
