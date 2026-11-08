# Laws

The rules this tool must never break. Each has a property test, `law_<name>`, run on generated projects and edits. If a test fails, fix the code. Changing a law needs the owner's ruling.

1. **Inverse** (`law_inverse`): applying any valid ops and then their inverses gives back the same project.
2. **Validity** (`law_validity`): a commit that lands leaves a project that passes validate.
3. **Drawable** (`law_drawable`): anything validate accepts renders with warnings at worst, and never panics.
4. **No lost work** (`law_no_lost_work`): a stale commit rebases or conflicts. It never overwrites a change it did not see.
5. **Order independence** (`law_order_independence`): commits with disjoint footprints give the same project in either order.
6. **Unbroken history** (`law_unbroken_history`): every commit links to its parent, every version is the sha256 of the bytes on disk, and a crash at any step leaves a project that opens.
7. **Honest authorship** (`law_honest_authorship`): an agent cannot land a sticky under another author, and nothing changes a stored sticky's author.
8. **Containment** (`law_containment`): no path reads or overwrites outside what it should, including through symlinks.
9. **Determinism** (`law_determinism`): the same project compiles to the same frames and renders the same pixels, and compiling always terminates.
10. **Format round trip** (`law_round_trip`): reading and rewriting a project keeps everything it held, including fields this version does not know.
