#!/usr/bin/env Rscript
# A press checked for what it moves: each picture of a state a press leads
# to, against the picture of the state before it, both taken by the probe
# (ONYX_SHOT) at the same size. For the left column (the settings' rail,
# or the sidebar with --split 272) and the page apart, it prints the box, in
# points, that holds every pixel that differs. A press that moves nothing
# differs only inside what it changed: a box that reaches a card's or a
# row's edge, or the page under the control, is a press that moved the
# layout (docs/probing.md, A press, checked).
#
#   Rscript scripts/press-diff.R [--split <points>] <before.png> <after.png>...
library(png)

args <- commandArgs(trailingOnly = TRUE)
split_points <- 190
if (length(args) >= 2 && args[1] == "--split") {
  split_points <- as.numeric(args[2])
  args <- args[-(1:2)]
}
if (length(args) < 2) stop("usage: press-diff.R [--split <points>] <before.png> <after.png>...")
before <- readPNG(args[1])

box <- function(d, x0) {
  if (!any(d)) return("same")
  r <- range(which(rowSums(d) > 0))
  c <- range(which(colSums(d) > 0))
  sprintf("x %4.0f-%4.0f, y %4.0f-%4.0f", (c[1] - 1 + x0) / 2, (c[2] + x0) / 2, (r[1] - 1) / 2, r[2] / 2)
}

for (f in args[-1]) {
  after <- readPNG(f)
  if (!identical(dim(after), dim(before))) {
    cat(sprintf("%-32s the window's size changed\n", basename(f)))
    next
  }
  d <- apply(abs(after - before), c(1, 2), max) > 0.02
  split <- split_points * 2
  cat(sprintf("%-32s left: %-28s page: %s\n", basename(f), box(d[, 1:split, drop = FALSE], 0), box(d[, (split + 1):ncol(d), drop = FALSE], split)))
}
