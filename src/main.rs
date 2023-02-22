use std::{mem::size_of, cell::Cell, any::Any, ptr::null_mut};

use sjlj::{ JumpBuf, setjmp, longjmp };
use stack::approximate_stack_pointer;

mod stack;

#[allow(dead_code)]
#[link(name = "gc", kind = "dylib")]
extern "C" {
    fn GC_malloc(size: usize) -> *mut u8;
    fn GC_free(ptr: *mut u8);
    fn GC_init();
}

thread_local! {
    static STACK_BOUNDS: stack::StackBounds = stack::StackBounds::current_thread_stack_bounds();
}

/// Returns the size of the native stack
fn stack_size() -> usize {
    STACK_BOUNDS.with(|bounds| {
        (bounds.origin as usize) - (approximate_stack_pointer() as usize)
    })
}

/// Returns the start of the native stack
fn stack_origin() -> usize {
    STACK_BOUNDS.with(|bounds| {
        bounds.origin as usize
    })
}


thread_local! {
    /// Used to store return value from invocation of continuation
    static CONT_VAL: Cell<Option<Box<dyn Any>>> = Cell::new(None);
}

struct Cont {
    /// saved continuation state
    state: JumpBuf,
    /// size of the captured stack
    csize: usize,
    /// start pointer of the native stack
    cstart: usize,
    /// end pointer of the native stack
    cend: usize,
    /// whether the continuation is fresh or not (was it invoked?)
    fresh: bool,
    /// pointer to the captured stack
    cstack: *mut u8
}

/// Captures current continuation and returns it. 
/// 
/// If continuation is invoked, it will return the value passed to continuation.
/// 
/// # Safety
/// 
/// Read [restore_cont_jump].
#[inline(never)]
unsafe fn make_continuation() -> Result<*mut Cont, Box<dyn Any>> {
    let addr = approximate_stack_pointer() as usize;

    let start_stack = stack_origin();

    let csize;
    let cstart;
    let cend;
    // compute the size of the stack and its end
    if addr < start_stack {
        csize = start_stack - addr;
        cstart = addr;
        cend = start_stack;
    } else {
        csize = addr - start_stack;
        cstart = start_stack;
        cend = addr;
    }

    let cont = GC_malloc(size_of::<Cont>()) as *mut Cont;
    (*cont).csize = csize;
    (*cont).cstart = cstart;
    (*cont).cend = cend;
    (*cont).fresh = true;
    (*cont).cstack = GC_malloc(csize);
    // copy native stack to the continuation
    libc::memcpy((*cont).cstack as _, cstart as _, csize);

    if setjmp(&mut (*cont).state) == 0 {
        // continuation is fresh, return it
        Ok(cont)
    } else {
        // continuation was invoked, return the value
        let val = CONT_VAL.with(|cell| cell.replace(None).unwrap());
        Err(val)
    }
}

/// Restores the continuation and jumps to it.
/// 
/// This code will recursively call itself until the stack size is large enough to fit the continuation.
/// 
/// # Safety
/// 
/// Inheretely unsafe, because it uses `longjmp` to jump to the continuation. All local variables that depend
/// on destructors will be broken.
#[inline(never)]
unsafe fn restore_cont_jump(k: *mut Cont) -> ! {
        let _unused_buf: [u8; 1000] = [0; 1000];

        let cur_stack_size = stack_size();
       
        if cur_stack_size <= ((*k).csize as usize + 1024) {
            restore_cont_jump(k);
        } else {
            (*k).fresh = false;
            libc::memcpy((*k).cstart as _, (*k).cstack as _, (*k).csize);
            //cstart.copy_from_nonoverlapping((*k).cstack, (*k).csize);
            longjmp(&(*k).state, 1);
        }
    
}

/// Restores the continuation and jumps to it.
/// 
/// # Safety
/// 
/// Read [`restore_cont_jump`].
unsafe fn restore_continuation<T: Any>(k: *mut Cont, value: T) -> ! {
    CONT_VAL.with(|cell| cell.set(Some(Box::new(value))));
    restore_cont_jump(k);
}

/// Calls the continuation `k` with `val`.
/// 
/// # Safety  
///
/// Read [`restore_cont_jump`].
unsafe fn call_cont<T: Any>(k: *mut Cont, val: T) -> ! {
    restore_continuation(k, val);
}


/// Calls `proc` with current continuation `k`.
/// If `k` is fresh, `proc` is called and the result is returned. Otherwise, the continuation is restored and the result is returned.
/// 
/// # Safety
/// 
/// Read [`restore_cont_jump`].
#[inline(never)]
unsafe fn call_cc<T: 'static>(proc: fn (*mut Cont) -> T) -> T {
    let k = make_continuation();

    match k {
        Ok(k) => {
            if (*k).fresh {
                proc(k)
            } else {
                restore_cont_jump(k);
            }
        },
        // continuation returned
        Err(val) => *val.downcast().unwrap()
    }
}   

static mut RET: *mut Cont = null_mut();

fn main() {
    unsafe {
        // we have to box the counter, otherwise call to continuation will always restore 
        // the same value of the counter. 
        let count = GC_malloc(size_of::<u32>()) as *mut u32;
        
        println!("{}", 100 + call_cc::<u32>(|k| {
            RET = k;
            call_cont(k, 100u32)
        }));
        
        if *count < 3 {
            *count += 1;
            println!("🚀");
            call_cont::<u32>(RET, *count);
        } 
    }
}