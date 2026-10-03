#!/bin/sh
# Usage: CPUS=1 DIR=pricing ./run.sh 'command'
exec docker run --rm --cpus=${CPUS:-1} --memory=${MEM:-512m} --network=none -v /tmp/et2/work:/work -w /work/${DIR:-pricing} mutarust-et:e212c49 sh -c "git config --global safe.directory '*'; $*"
