# License

Auberge is licensed under the **GNU Affero General Public License v3.0 (AGPL-3.0)**.

## What This Means

### You Can

✓ **Use** Auberge for personal or commercial purposes
✓ **Modify** the source code
✓ **Distribute** copies of Auberge
✓ **Distribute** modified versions

### You Must

✓ **Disclose source** when distributing
✓ **License modifications** under AGPL-3.0
✓ **State changes** made to the code
✓ **Include license and copyright** notices
✓ **Provide source** if running as network service (AGPL network clause)

### You Cannot

✗ **Sublicense** under different terms
✗ **Hold liable** the authors or copyright holders

## AGPL vs GPL

The **AGPL** includes an additional requirement:

If you modify Auberge and run it as a network service (e.g., SaaS), you must make your modified source code available to users of that service.

**Example:**

- You modify Auberge to add features
- You offer it as a hosted service
- Users interact with it over a network
- **You must provide your source code** to those users

This prevents "SaaS loophole" where modifications aren't shared.

## Why AGPL?

Auberge uses AGPL to:

1. **Keep software free:** Ensure modifications remain open source
2. **Prevent proprietary forks:** No closed-source commercial versions
3. **Support community:** Modifications benefit everyone
4. **Close SaaS loophole:** Network services must share source

## Commercial Use

**You can use Auberge commercially** as long as:

- You comply with AGPL terms
- You share modifications if distributing or offering as service
- You don't create proprietary forks

**Examples:**

✓ **Allowed:**

- Using Auberge to manage VPS for your business
- Offering consulting services around Auberge
- Packaging Auberge for a Linux distribution

✗ **Not allowed:**

- Creating proprietary SaaS based on Auberge without sharing code
- Distributing modified closed-source versions
- Sublicensing under non-AGPL terms

## Dual Licensing

Auberge is **not available** under dual licensing.

If you need different licensing terms:

- Consider other VPS management tools
- Contact maintainer for discussion (no guarantees)

## Contributing

By contributing to Auberge, you agree:

- Your contributions are licensed under AGPL-3.0
- You have the right to contribute the code
- You grant the project a perpetual license to use your contribution

## Full License Text

The complete AGPL-3.0 license is available:

- In the repository: [LICENSE](https://github.com/sripwoud/auberge/blob/main/LICENSE)
- Online: [gnu.org/licenses/agpl-3.0.html](https://www.gnu.org/licenses/agpl-3.0.html)

## Questions?

**Not sure if your use case complies?**

- Read the [full license text](https://www.gnu.org/licenses/agpl-3.0.html)
- Consult a lawyer (this is not legal advice)
- Ask in [GitHub Discussions](https://github.com/sripwoud/auberge/discussions)

## Other Project Licenses

Auberge depends on third-party software with various licenses:

- **Rust crates:** See `Cargo.toml` and individual crate licenses
- **Ansible collections:** Various open source licenses
- **Applications deployed:** Each has its own license (check role README)

## Related Pages

- [Contributing](development/contributing.md) - How to contribute
- [Architecture Decisions](about/architecture-decisions.md) - Why AGPL
