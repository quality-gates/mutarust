#!/bin/sh
# Run "$@" while sampling /tmp disk use; stop it above 3 GB. Peak goes to /work/peak.txt.
"$@" & pid=$!; peak=0
while kill -0 $pid 2>/dev/null; do
  kb=$(du -sk /tmp 2>/dev/null | cut -f1); [ "$kb" -gt "$peak" ] && peak=$kb
  [ "$kb" -gt 3000000 ] && { echo "GUARD: /tmp over 3 GB, stopping" ; kill -INT $pid; }
  sleep 1
done
wait $pid; rc=$?; echo "PEAK_TMP_KB=$peak"; return $rc 2>/dev/null || exit $rc
