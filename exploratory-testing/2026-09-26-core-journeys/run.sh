#!/bin/sh
exec docker run --rm --cpus=1 --memory=512m --network=none -v /tmp/et-mutarust/work:/work -w /work/pricing mutarust-et:406beab sh -c "$*"
