#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
"""Rebuild original food materials without requiring any scanned characters."""
import argparse
import runpy
from pathlib import Path

parser=argparse.ArgumentParser()
parser.add_argument('--size',type=int,default=2048)
args=parser.parse_args()
module=runpy.run_path(str(Path(__file__).with_name('build_visual_assets.py')))
module['build_food_maps'].__globals__['SIZE']=args.size
module['build_food_maps']()
print(f'Original food materials generated at {args.size} x {args.size}')
