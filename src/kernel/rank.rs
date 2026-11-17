//! How central a piece of code is to the work at hand: personalized
//! PageRank over the call graph. A function that the code being worked on
//! calls, or that calls it, gets a share of its weight; a share flows on to
//! what that one calls, and so on, fading at every step. What the work
//! touches (changed files, the code closest in meaning to the goal) is
//! where every walk starts again.

/// Personalized PageRank: `edges` from caller to callee over `n` nodes,
/// `personal` how much each node is where walks start again (any scale; all
/// zero: everywhere alike). The ranks add up to 1.
pub fn pagerank(n: usize, edges: &[(usize, usize)], personal: &[f64], damping: f64, iterations: usize) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    let total: f64 = personal.iter().take(n).filter(|x| x.is_finite() && **x > 0.0).sum();
    let start: Vec<f64> = (0..n)
        .map(|i| {
            let w = personal.get(i).copied().filter(|x| x.is_finite() && *x > 0.0).unwrap_or(0.0);
            if total > 0.0 { w / total } else { 1.0 / n as f64 }
        })
        .collect();
    let mut out_degree = vec![0usize; n];
    for &(a, b) in edges {
        if a < n && b < n {
            out_degree[a] += 1;
        }
    }
    let mut rank = start.clone();
    for _ in 0..iterations {
        let mut next: Vec<f64> = start.iter().map(|s| (1.0 - damping) * s).collect();
        // what has nowhere to go starts again where the work is
        let dangling: f64 = (0..n).filter(|&i| out_degree[i] == 0).map(|i| rank[i]).sum();
        for (i, x) in next.iter_mut().enumerate() {
            *x += damping * dangling * start[i];
        }
        for &(a, b) in edges {
            if a < n && b < n {
                next[b] += damping * rank[a] / out_degree[a] as f64;
            }
        }
        rank = next;
    }
    rank
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/kernel/rank.rs"]
mod tests;
