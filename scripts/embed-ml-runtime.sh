#!/usr/bin/env bash
set -euo pipefail
cd "$SRCROOT/../.."
bash scripts/build-ml-runtime.sh
framework_directory="$TARGET_BUILD_DIR/$CONTENTS_FOLDER_PATH/Frameworks"
resource_directory="$TARGET_BUILD_DIR/$UNLOCALIZED_RESOURCES_FOLDER_PATH"
mkdir -p "$framework_directory" "$resource_directory/SubjectRuntimeLicenses"
cp build/subject-runtime/libstrata_onnxruntime.dylib "$framework_directory/"
code_sign_identity="$EXPANDED_CODE_SIGN_IDENTITY"
if [[ -z "$code_sign_identity" ]]; then code_sign_identity="-"; fi
codesign --force --sign "$code_sign_identity" "$framework_directory/libstrata_onnxruntime.dylib"
shasum -a 256 "$framework_directory/libstrata_onnxruntime.dylib" | awk '{print $1}' > "$resource_directory/subject-runtime.sha256"
cp vendor/onnxruntime/LICENSE vendor/onnxruntime/ThirdPartyNotices.txt vendor/onnxruntime/NOTICE.md "$resource_directory/SubjectRuntimeLicenses/"
