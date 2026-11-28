#!/usr/bin/env Rscript
# The App Store's page for a version of Onyx, written from the repository
# through the App Store Connect API: the version (Cargo.toml's), the build
# release-ios.sh sent, the words in each language
# (crates/onyx-app/ios/store/<locale>/*.txt) and the pictures store-shots.R
# made (dist/store/). It writes the version that is being prepared and
# makes one when there is none; nothing goes to review without --submit.
#
#   Rscript scripts/store-ios.R [--no-pictures] [--submit]
#
# The key is release-ios.sh's: ASC_KEY_ID, ASC_ISSUER_ID and the file in
# ~/.appstoreconnect/private_keys/ (docs/release.md, One-time setup). A
# language's folder holds name.txt, subtitle.txt, description.txt,
# keywords.txt, promotional_text.txt, support_url.txt, privacy_url.txt and,
# from the second version on, release_notes.txt (What's New). The pictures
# go to the app's first language, and the others show the same ones.
# What is set once on the store's own pages stays there: the category, the
# age rating, the price, the countries, the privacy answers, the review's
# contact (docs/release.md, An iPhone's and an iPad's release).
suppressMessages({
  library(httr2)
  library(jsonlite)
})

args <- commandArgs(FALSE)
setwd(file.path(dirname(normalizePath(sub("^--file=", "", grep("^--file=", args, value = TRUE)))), ".."))
flags <- commandArgs(TRUE)
bundle <- "com.alkalinesoftware.onyx"
store <- "crates/onyx-app/ios/store"
version <- sub('^version = "(.*)"$', "\\1", grep('^version = "', readLines("Cargo.toml"), value = TRUE)[1])

key_id <- Sys.getenv("ASC_KEY_ID")
issuer <- Sys.getenv("ASC_ISSUER_ID")
key_file <- path.expand(sprintf("~/.appstoreconnect/private_keys/AuthKey_%s.p8", key_id))
if (key_id == "" || issuer == "" || !file.exists(key_file)) stop("store-ios: no App Store Connect key (ASC_KEY_ID, ASC_ISSUER_ID and its file; docs/release.md, One-time setup)")
key <- openssl::read_key(key_file)

# A token lasts twenty minutes at most, so one is made for every ten.
made <- 0
bearer <- ""
token <- function() {
  now <- as.integer(Sys.time())
  if (now - made > 600) {
    bearer <<- jose::jwt_encode_sig(jose::jwt_claim(iss = issuer, iat = now, exp = now + 1100L, aud = "appstoreconnect-v1"), key, header = list(kid = key_id, typ = "JWT"))
    made <<- now
  }
  bearer
}

# One call. A body is the `data` of the request; what comes back is read as lists.
api <- function(method, path, data = NULL) {
  r <- request(paste0("https://api.appstoreconnect.apple.com", path)) |> req_method(method) |> req_auth_bearer_token(token()) |> req_error(is_error = \(r) FALSE)
  if (!is.null(data)) r <- req_body_raw(r, toJSON(list(data = data), auto_unbox = TRUE, null = "null"), "application/json")
  p <- req_perform(r)
  body <- if (resp_has_body(p)) resp_body_json(p) else list()
  if (resp_status(p) >= 400) stop("store-ios: ", method, " ", path, ": ", paste(vapply(body$errors, \(e) paste(e$title, e$detail), ""), collapse = "; "), call. = FALSE)
  invisible(body)
}
one <- function(type, id) list(data = list(type = type, id = id))
words <- function(locale, name) {
  f <- file.path(store, locale, paste0(name, ".txt"))
  if (file.exists(f)) trimws(paste(readLines(f, warn = FALSE, encoding = "UTF-8"), collapse = "\n")) else NULL
}

app <- api("GET", paste0("/v1/apps?filter[bundleId]=", bundle))$data[[1]]
locales <- basename(Filter(\(d) file.exists(file.path(d, "description.txt")), list.dirs(store, recursive = FALSE)))

# The version being prepared, made when the last one is already out.
open <- c("PREPARE_FOR_SUBMISSION", "DEVELOPER_REJECTED", "REJECTED", "METADATA_REJECTED", "INVALID_BINARY")
versions <- api("GET", sprintf("/v1/apps/%s/appStoreVersions?filter[platform]=IOS", app$id))$data
mine <- Filter(\(v) v$attributes$appVersionState %in% open, versions)
attributes <- list(versionString = version, copyright = paste(format(Sys.Date(), "%Y"), "Alkaline Software"))
if (length(mine)) {
  mine <- mine[[1]]
  api("PATCH", paste0("/v1/appStoreVersions/", mine$id), list(type = "appStoreVersions", id = mine$id, attributes = attributes))
} else {
  mine <- api("POST", "/v1/appStoreVersions", list(type = "appStoreVersions", attributes = c(list(platform = "IOS"), attributes), relationships = list(app = one("apps", app$id))))$data
}
cat("version", version, "\n")

# The newest build of this version that Apple has finished with.
builds <- api("GET", sprintf("/v1/builds?filter[app]=%s&filter[preReleaseVersion.version]=%s&filter[processingState]=VALID&filter[expired]=false&sort=-uploadedDate&limit=1", app$id, version))$data
if (!length(builds)) stop("store-ios: no build of ", version, " is ready there (scripts/release-ios.sh --upload, then a wait)")
api("PATCH", sprintf("/v1/appStoreVersions/%s/relationships/build", mine$id), list(type = "builds", id = builds[[1]]$id))
cat("build", builds[[1]]$attributes$version, "\n")

# The version's words, and the app's (its name, its line and its privacy page), a language at a time.
info <- api("GET", sprintf("/v1/apps/%s/appInfos", app$id))$data
info <- c(Filter(\(i) !identical(i$attributes$state, "READY_FOR_DISTRIBUTION"), info), info)[[1]]
there <- api("GET", sprintf("/v1/appStoreVersions/%s/appStoreVersionLocalizations", mine$id))$data
put <- function(type, existing, locale, attributes, parent) {
  attributes <- Filter(Negate(is.null), attributes)
  at <- Filter(\(l) l$attributes$locale == locale, existing)
  if (length(at)) {
    api("PATCH", sprintf("/v1/%s/%s", type, at[[1]]$id), list(type = type, id = at[[1]]$id, attributes = attributes))
    at[[1]]$id
  } else {
    api("POST", paste0("/v1/", type), list(type = type, attributes = c(list(locale = locale), attributes), relationships = parent))$data$id
  }
}
pages <- list()
for (locale in locales) {
  pages[[locale]] <- put("appStoreVersionLocalizations", there, locale, list(description = words(locale, "description"), keywords = words(locale, "keywords"), promotionalText = words(locale, "promotional_text"), supportUrl = words(locale, "support_url"), marketingUrl = words(locale, "marketing_url"), whatsNew = words(locale, "release_notes")), list(appStoreVersion = one("appStoreVersions", mine$id)))
  # A language new to the version has just been given the app's words too, empty: asked for after it.
  named <- api("GET", sprintf("/v1/appInfos/%s/appInfoLocalizations", info$id))$data
  put("appInfoLocalizations", named, locale, list(name = words(locale, "name"), subtitle = words(locale, "subtitle"), privacyPolicyUrl = words(locale, "privacy_url")), list(appInfo = one("appInfos", info$id)))
  cat("words", locale, "\n")
}

# The pictures, each set emptied and filled again in the files' order.
if (!"--no-pictures" %in% flags) {
  first <- app$attributes$primaryLocale
  if (is.null(pages[[first]])) stop("store-ios: no words for the app's first language, ", first)
  sets <- api("GET", sprintf("/v1/appStoreVersionLocalizations/%s/appScreenshotSets", pages[[first]]))$data
  for (kind in list(c("iphone", "APP_IPHONE_67"), c("ipad", "APP_IPAD_PRO_3GEN_129"))) {
    files <- sort(list.files(file.path("dist/store", kind[1]), "\\.png$", full.names = TRUE))
    if (!length(files)) stop("store-ios: no pictures in dist/store/", kind[1], " (Rscript scripts/store-shots.R)")
    set <- Filter(\(s) s$attributes$screenshotDisplayType == kind[2], sets)
    set <- if (length(set)) set[[1]]$id else api("POST", "/v1/appScreenshotSets", list(type = "appScreenshotSets", attributes = list(screenshotDisplayType = kind[2]), relationships = list(appStoreVersionLocalization = one("appStoreVersionLocalizations", pages[[first]]))))$data$id
    for (old in api("GET", sprintf("/v1/appScreenshotSets/%s/appScreenshots", set))$data) api("DELETE", paste0("/v1/appScreenshots/", old$id))
    for (f in files) {
      bytes <- readBin(f, "raw", file.size(f))
      shot <- api("POST", "/v1/appScreenshots", list(type = "appScreenshots", attributes = list(fileName = basename(f), fileSize = length(bytes)), relationships = list(appScreenshotSet = one("appScreenshotSets", set))))$data
      for (part in shot$attributes$uploadOperations) {
        r <- request(part$url) |> req_method(part$method) |> req_body_raw(bytes[part$offset + seq_len(part$length)])
        for (h in part$requestHeaders) r <- req_headers(r, !!h$name := h$value)
        req_perform(r)
      }
      api("PATCH", paste0("/v1/appScreenshots/", shot$id), list(type = "appScreenshots", id = shot$id, attributes = list(uploaded = TRUE, sourceFileChecksum = unname(tools::md5sum(f)))))
      cat("picture", f, "\n")
    }
  }
}

# To review: a submission with this version in it, sent.
if ("--submit" %in% flags) {
  sub <- api("POST", "/v1/reviewSubmissions", list(type = "reviewSubmissions", attributes = list(platform = "IOS"), relationships = list(app = one("apps", app$id))))$data
  api("POST", "/v1/reviewSubmissionItems", list(type = "reviewSubmissionItems", relationships = list(reviewSubmission = one("reviewSubmissions", sub$id), appStoreVersion = one("appStoreVersions", mine$id))))
  api("PATCH", paste0("/v1/reviewSubmissions/", sub$id), list(type = "reviewSubmissions", id = sub$id, attributes = list(submitted = TRUE)))
  cat("sent to review:", version, "\n")
}
