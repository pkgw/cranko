# `cranko git-util reboot-branch`

This command resets this history of a Git branch to be a single commit
containing a specified tree of files. It can be useful to update [GitHub
Pages][gh-pages] or similar services that publish content based on a Git branch
that whose history is unimportant.

[gh-pages]: https://pages.github.com/

#### Usage

```
cranko git-util reboot-branch [-m {MESSAGE}] {BRANCH} {ROOTDIR}
```

Rewrites the local version of the Git branch `{BRANCH}` to contain a single
commit whose contents are those of the directory `{ROOTDIR}`. If specified,
`{MESSAGE}` is used as the Git commit message. The commit author is generic.

The history of the named branch is completely obliterated. If it is to be pushed
to any remotes, it will need to be force-pushed.

#### Example

```shell
# During CI build/test of `rc` commit:
$ ./website/generate.sh
# After release is locked in:
$ cranko git-util reboot-branch gh-pages ./website/content/
$ git push -f origin gh-pages
```
