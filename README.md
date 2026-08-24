# fxlfit

Preflight a fixed-layout EPUB -- a comic, a manga volume, a picture book --
before you upload it. Offline Rust CLI.

A fixed-layout book is only fixed-layout because the package document says so,
every page is only the right size because it declares a viewport that matches
the canvas it was drawn on, and to a reader using a screen reader the whole
book is whatever the alt text says it is. Those three things are invisible in
every preview you have. They surface in a store rejection, or in a one-star
review from someone who bought a book that turned out to be blank to them.

fxlfit reads the container and reports what is actually in it.

```
$ fxlfit episode07.epub
fxlfit: fixed-layout preflight
book:          episode07.epub (48.2 MiB on disk, 49.1 MiB uncompressed)
package:       OEBPS/content.opf (EPUB 3.0)
title:         ある日の商店街 第7話 [ja]
layout:        pre-paginated, 32 of 32 spine page(s) pre-paginated, viewport 1200x1800
spread:        rendition:spread both, page progression rtl
images:        32 referenced, 0 with a text alternative
a11y metadata: conformsTo (none), certifiedBy (none)
budgets:       image 5.0 MiB, total unlimited, min-scale 1.00x, aspect tolerance 2.0%, target WCAG 2.2 Level AA

findings: 2 error(s), 4 warning(s), 0 note(s)

error  A11Y001  32 image(s) carry no text alternative
                OEBPS/text/p001.xhtml (page 1): p001.jpg, OEBPS/text/p002.xhtml (page 2): p002.jpg and 30 more
                fix: give every content image an alt attribute (or, for an SVG page, a <title> the image is described by); on a comic page the image is the page, so a missing alt leaves the reader with nothing
                ref: https://www.w3.org/TR/epub-a11y-11/#sec-conf-content

warn   IMG003   3 page image(s) are smaller than their viewport
                OEBPS/text/p014.xhtml (page 14): image 900x1350 into viewport 1200x1800 (0.75x), ...
                fix: export the artwork at least at viewport size (currently flagged below 1.00x); a reading system upscales what it is given, and line art shows it first
                ref: https://w3c.github.io/epub-specs/wg-notes/fxl-a11y-tech/

verdict: NOT READY (2 error(s), 4 warning(s), 0 note(s))
```

Everything runs locally. Nothing is uploaded, no network call is made, and no
image is ever decoded -- only image headers are read, so a 12000 x 18000 page
costs the same as a thumbnail.

## What this is not

It is not an EPUB validator. [epubcheck](https://github.com/w3c/epubcheck)
decides whether a file is conformant, and you should keep running it. fxlfit
assumes the file is valid and asks the questions that come after that:

- Is the book actually fixed-layout, or does it just look like one in your
  authoring tool?
- Does every page declare the canvas it was drawn on, and is the artwork that
  size?
- Is anything in the container that nothing references, and is any page over
  the budget you have to hit?
- Is there anything at all for a reader who cannot see the pages, and does the
  metadata say what is actually true?

It is also not an accessibility audit.
[Ace by DAISY](https://daisy.github.io/ace/) runs WCAG rules over the rendered
content and is the right tool for that. fxlfit looks at the things that are
specific to a picture book or a comic: whether the alt text on a page is a
description or a filename, whether the same sentence was pasted onto thirty
pages, and whether the discovery metadata claims more than the pages deliver.
Neither tool, and no tool, can tell you whether a description is any good. Read
them.

## Install

```bash
git clone https://github.com/printemps-tokyo/fxlfit
cd fxlfit
cargo build --release
# the binary is at ./target/release/fxlfit
```

Or take a prebuilt tarball from the
[releases page](https://github.com/printemps-tokyo/fxlfit/releases); every
artifact ships with a `.sha256` next to it.

## Quickstart

```bash
# The whole report
fxlfit book.epub

# What each page declares and what it actually holds
fxlfit book.epub --pages

# Only the accessibility side, failing on anything at all
fxlfit book.epub --only access --fail-on warn

# In CI, with the budgets your store actually imposes
fxlfit book.epub --max-image-bytes 5MiB --max-total-bytes 300MiB --json
```

`--pages` prints the spine in reading order, which is usually where a problem
becomes obvious:

```
   #  document                           layout     viewport    image                          pixels          bytes  alt
   1  OEBPS/text/p001.xhtml              fixed      1200x1800   p001.jpg                       1200x1800     1.2 MiB  yes
   2  OEBPS/text/p002.xhtml              fixed      1200x1800   p002.jpg                       1200x1800     1.4 MiB  yes
   3  OEBPS/text/p003.xhtml              fixed      -           p003.jpg                       1200x1800     1.1 MiB  MISSING
   4  OEBPS/text/p004.xhtml              fixed      1200x1800   p004.jpg                       600x900       312 KiB  yes
   5  OEBPS/text/p005.xhtml              fixed      2400x1800   p005.jpg                       2400x1800     2.6 MiB  yes
```

## What it checks

Run `fxlfit --list-checks` for the current catalogue with severities. Every
check carries the primary source it comes from, printed with the finding.

| family | what it is about |
| --- | --- |
| `PKG` | package identity, navigation document, cover, resources that are missing, remote or unreferenced, encryption |
| `FXL` | `rendition:layout`, per-page viewports and how consistent they are, `rendition:spread` and page-spread sides, `page-progression-direction`, deprecated and conflicting rendition properties |
| `IMG` | images missing from the container, per-image and whole-book size budgets, artwork smaller than its viewport, artwork far larger than its viewport, media types |
| `A11Y` | alt text presence and shape, pages hidden from assistive technology, EPUB Accessibility 1.1 discovery metadata, the conformance string and its certifier, metadata that contradicts the pages, per-document language and title, page-list and table of contents |

Two of these are worth spelling out, because they are the ones a generic tool
does not do.

Alt text is judged on shape, not only on presence. `alt="p012.png"`,
`alt="page 12"` and the same sentence repeated across every page all pass a
presence check and are all reported here, with the string that was found.
Length alone is never used as evidence: a good description can be short in
Japanese.

Discovery metadata is compared with the pages. A book that declares
`schema:accessibilityFeature: alternativeText` while eight of its pages have no
alt, or `schema:accessModeSufficient: textual` for a book that is entirely
images, is reported -- an overstated claim is worse than an honest `none`,
because a reader chooses the book on the strength of it. Since the European
Accessibility Act took effect on 2025-06-28, that metadata is also what an EU
storefront has to display.

## Fixed-layout, declared or not

If the package does not declare `rendition:layout: pre-paginated` but the pages
are unmistakably fixed-layout -- most of the spine is a single image with a
viewport -- fxlfit reports the missing declaration and then runs the layout
checks against those pages anyway. Hiding thirty findings behind one missing
line would make the report useless exactly when it is most needed.

## Budgets are yours to set

There is no built-in table of store limits. Per-image and per-book size ceilings
differ by retailer, change without notice, and none of them are in a spec, so
they are flags with documented defaults rather than claims this tool makes on a
store's behalf:

```bash
fxlfit book.epub --max-image-bytes 5MiB --max-total-bytes 650MiB
```

The same applies to the accessibility target. `--wcag` and `--level` say what
the conformance string is measured against; the defaults are WCAG 2.2 Level AA,
which is what EN 301 549 -- the standard the European Accessibility Act is read
through -- points at.

## In CI

```yaml
- run: fxlfit dist/book.epub --fail-on warn --max-image-bytes 5MiB
```

Exit codes:

| code | meaning |
| --- | --- |
| `0` | nothing at or above the `--fail-on` threshold (default: `error`) |
| `1` | findings reached the threshold |
| `2` | the file could not be read, or the arguments were wrong |

`--fail-on never` reports without ever failing the build. `--only` and `--skip`
take check ids (`FXL003`), family prefixes (`A11Y`) or categories (`layout`,
`access`, `assets`, `package`), so a repository can adopt the tool one family at
a time.

## JSON

`--json` prints the same facts with no formatting decisions in them: the
verdict and counts, the budgets used, every page with its viewport, images,
pixel sizes and alt text, and every finding with its id, severity, locations,
fix and reference.

```bash
fxlfit book.epub --json | jq '.findings[] | select(.severity == "error") | .id'
```

## Limits, stated plainly

- Images referenced only from CSS (`background-image`) are not seen, and
  neither is anything a script builds at runtime. A page that hides its artwork
  in a stylesheet looks empty to this tool -- and to an accessibility tree.
- Encrypted resources are opaque. If `META-INF/encryption.xml` is present, that
  is reported and the affected resources are not inspected.
- Whether an alt text is a good description, whether the reading order inside a
  page is right, and whether the colours have enough contrast are all human
  judgements. This tool does not make them and does not pretend to.
- Media types are compared against the file extension, not against a full
  decode.

## Sources

Rules are written against primary sources, read on 2026-08-24:

- [EPUB 3.3](https://www.w3.org/TR/epub-33/), in particular
  [fixed-layout properties](https://www.w3.org/TR/epub-33/#sec-fixed-layouts)
  and the [core media types](https://www.w3.org/TR/epub-33/#sec-core-media-types)
- [EPUB Accessibility 1.1](https://www.w3.org/TR/epub-a11y-11/) -- discovery
  metadata, the conformance string and the certifier properties
- [EPUB Accessibility - EU Accessibility Act Mapping](https://www.w3.org/TR/epub-a11y-eaa-mapping/)
- [Fixed-Layout Accessibility Techniques](https://w3c.github.io/epub-specs/wg-notes/fxl-a11y-tech/)
  (W3C Working Group note)

## License

MIT. See [LICENSE](LICENSE).
