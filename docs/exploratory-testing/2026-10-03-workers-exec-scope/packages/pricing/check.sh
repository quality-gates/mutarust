#!/bin/sh
echo "ORIG=$MUTATE_ORIGINAL CHANGED=$MUTATE_CHANGED PKG=$MUTATE_PACKAGE TO=$MUTATE_TIMEOUT REC=$TEST_RECURSIVE V=$MUTATE_VERBOSE D=$MUTATE_DEBUG PWD=$PWD" >> /work/exec-env.log
if cargo test --offline -q >/dev/null 2>&1; then exit 1; else exit 0; fi
