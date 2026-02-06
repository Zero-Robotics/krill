#!/usr/bin/env python3
"""Setup script for krill-python SDK."""

from setuptools import setup

with open("README.md", "r", encoding="utf-8") as f:
    long_description = f.read()

setup(
    name="krill-sdk",
    version="0.1.0",
    description="Python client library for Krill process orchestrator",
    long_description=long_description,
    long_description_content_type="text/markdown",
    author="Tommaso Pardi",
    author_email="",  # Add your email if desired
    license="Apache-2.0",
    license_files=["../../LICENSE.md"],
    py_modules=["krill"],
    python_requires=">=3.7",
    classifiers=[
        "Development Status :: 4 - Beta",
        "Intended Audience :: Developers",
        "License :: OSI Approved :: Apache Software License",
        "Operating System :: POSIX :: Linux",
        "Operating System :: MacOS :: MacOS X",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.7",
        "Programming Language :: Python :: 3.8",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
        "Programming Language :: Python :: 3.12",
        "Topic :: Software Development :: Libraries :: Python Modules",
        "Topic :: System :: Monitoring",
    ],
    keywords="robotics orchestration monitoring heartbeat ipc",
    url="https://github.com/your-org/krill",
)
