#!/usr/bin/env Rscript
# A finger on Onyx in a simulator, through UIKit as a real one goes
# (docs/probing.md, A finger through UIKit): the app in the simulator is
# started by a UI test (scripts/touch/) with the ONYX_ variables set here,
# and the steps are done by XCTest's own touches, which pass through the
# system's hit test, its gesture recognizers and its menus.
#   Rscript scripts/touch.R <sim> <steps> [ONYX_VAR=value ...]
# Steps are split by ";": tap x,y | double x,y | hold x,y[,ms] |
# drag x,y,tx,ty[,held ms] | holddrag x,y,tx,ty[,held ms] | wait ms |
# type words | shot <file.png> | menu | pick <item>. Points from the
# screen's top left. The app must be installed (scripts/ios-app.sh).
a <- commandArgs(TRUE)
if (length(a) < 2) stop("usage: touch.R <sim> <steps> [ONYX_VAR=value ...]")
repo <- normalizePath(file.path(dirname(sub("--file=", "", grep("--file=", commandArgs(FALSE), value = TRUE))), ".."))
sim <- a[1]
steps <- strsplit(a[2], ";", fixed = TRUE)[[1]]
vars <- a[-(1:2)]
work <- file.path(tempdir(), "touch")
dir.create(work, showWarnings = FALSE)
built <- file.path(work, "dd")
run <- list.files(file.path(built, "Build", "Products"), "\\.xctestrun$", full.names = TRUE)
fresh <- length(run) == 0 || file.mtime(run[1]) < max(file.mtime(list.files(file.path(repo, "scripts", "touch"), recursive = TRUE, full.names = TRUE)))
# By its identifier: xcodebuild finds no device by a name on an older system.
devs <- do.call(c, unname(lapply(jsonlite::fromJSON(paste(system2("xcrun", c("simctl", "list", "devices", "-j"), stdout = TRUE), collapse = ""))$devices, function(d) if (length(d) && nrow(d)) setNames(d$udid, d$name))))
if (is.na(devs[sim])) stop("no simulator named ", sim)
dest <- paste0("platform=iOS Simulator,id=", devs[[sim]])
if (fresh) {
  proj <- file.path(repo, "scripts", "touch")
  system2("xcodegen", c("generate", "--spec", shQuote(file.path(proj, "project.yml")), "--project", shQuote(proj)), stdout = FALSE)
  ok <- system2("xcodebuild", c("build-for-testing", "-project", shQuote(file.path(proj, "Touch.xcodeproj")), "-scheme", "Touch", "-destination", shQuote(dest), "-derivedDataPath", shQuote(built), "-quiet"), stdout = FALSE, stderr = FALSE)
  if (ok != 0) stop("the touch runner did not build")
}
file <- file.path(work, "steps.txt")
writeLines(steps, file)
env <- c(paste0("TEST_RUNNER_TOUCH_STEPS=", file), paste0("TEST_RUNNER_TOUCH_", vars))
out <- system2("env", c(shQuote(env), "xcodebuild", "test-without-building", "-project", shQuote(file.path(repo, "scripts", "touch", "Touch.xcodeproj")), "-scheme", "Touch", "-destination", shQuote(dest), "-derivedDataPath", shQuote(built)), stdout = TRUE, stderr = TRUE)
said <- grep("^touch |error|failed|passed", out, value = TRUE)
cat(said, sep = "\n")
