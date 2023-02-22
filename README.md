# call/cc 

Simple implementation of call/cc using setjmp/longjump. Useful for implementing Scheme VMs. Not useful for using in Rust since it is instant UB. 
You can look into `src/main.rs` to see how it is used and implemented. `src/stack.rs` contains code to get stack bounds for current platform.