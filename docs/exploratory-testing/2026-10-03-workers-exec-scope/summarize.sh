#!/bin/sh
# Print "mutator line-content" for each escaped mutant from mutarust output
awk '/^escaped/{m=$3; f=$2} /^\+[^+]/{print f, m, $0}' 
