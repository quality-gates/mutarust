#!/bin/sh
cmp -s "$MUTATE_ORIGINAL" "$MUTATE_CHANGED" && echo SAME >> /work/code.log
diff "$MUTATE_ORIGINAL" "$MUTATE_CHANGED" | grep -c '^[<>]' >> /work/code.log
case "$CODE" in sleep) sleep 30; exit 0;; *) exit "$CODE";; esac
