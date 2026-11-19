// src/lib.rs

pub mod lexer;
pub mod compiler;
pub mod proofgen;

#[cfg(feature = "inquisitor")]
pub mod inquisitor;
#[cfg(feature = "inquisitor")]
pub mod inquisitor_model;