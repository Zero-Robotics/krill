# Contributing to Krill

First off, thank you for taking the time to contribute! It’s people like you who make Krill a better tool for everyone.

## Project Structure
Understanding where things live will help you get started:
* **/core**: The main Rust engine and logic.
* **/sdks**: Implementation for Rust, Python, and C++ interfaces.
* **/backends**: Execution logic (pixi, ros2, shell, and Docker).

---

## Getting Started

### Prerequisites
* **Rust 1.70+**
* **Unix-like OS** (Linux or macOS)
* **[Just](https://github.com/casey/just)** (our command runner)

### Setup
1. **Fork and clone** the repository.
2. **Verify the environment**:
   ```bash
   just check   # runs fmt-check, lint, and test
   just fmt
   ```
### Development Workflow
We use just to keep commands consistent. Please run these frequently during development:
* **just fmt**: Formats code according to project style
* **just lint**: Runs Clippy to catch common Rust mistakes
* **just test**: Runs the full test suite
* **just doc**: Builds and opens the local documentation

##🚀 What to Contribute
- Bug fixes: Found a bug? Open an issue first, then submit a PR.
- Tests: Additional test coverage is always appreciated.
- Documentation: Improvements to docs, examples, and error messages.
- Execution Backends: Community backends like Docker (now in the Open Edition!), pixi, ros2, or shell.
- SDK Improvements: Better ergonomics for the Rust, Python, and C++ SDKs.
- New Health Checks: TCP, HTTP, script, or custom checkers.

### Scope Boundaries
To keep the open-core model sustainable, please note:
* **Fleet Management**: This feature is reserved for Krill Pro/Enterprise and is not accepted as a community contribution.
* **Unsure?** If you're not sure if a feature fits the Open Edition, open an issue for discussion before you start coding!

### Submitting a Pull Request
- Branching: Create a feature branch from main.
- Commits: Make small, focused commits with descriptive messages.
- Validation: Ensure just check passes locally.
- Tests: Include tests for any new functionality.
- Template: Fill out the PR template provided when you open the request.

### Code Guidelines
- Safety First: Keep shell command validation strict. Safety is a core design principle.
- No Panics: Prefer returning Result types over panicking.
- Documentation: All new public APIs must be documented.
- Consistency: Follow existing patterns and naming conventions in the codebase.
