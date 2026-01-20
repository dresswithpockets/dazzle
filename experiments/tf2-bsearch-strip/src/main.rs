//! TF2 segfaults when performing the strip -> connected graphs -> self-merge procedure on every VPK. This experiment
//! applies the same process to a subset of all PCFs iteratively. I've tested this against summer2024_unusuals.pcf 
//! without a segfault, so there must a subset of all PCFs for which this causes a crash. I'll use the information as 
//! heuristic for patching & testing.
//! 
//! Given a lower bound of 1, an upper bound of N PCFs, and a current value of X=N/2: Run the process on X PCFs, if a
//! failure is detected, the next X will be halfway between the lower bound and the current X. If a failure isn't
//! detected, the next X will be halfway between the upper bound and the current X.
//! 

fn main() {
    todo!();
}
