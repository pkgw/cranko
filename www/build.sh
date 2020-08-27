#! /bin/bash
# Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
# Licensed under the MIT License.

# A very simple script to build static GitHub Pages content. This will
# probably be superseded pretty soon.

set -euo pipefail

cd "$(dirname $0)"

version="$(cranko show version cranko)"
sed -e "s/@VERSION@/${version}/g" fetch-tgz.sh.in >content/fetch-latest.sh
