# AE SDK License Note (2026-07-13)

Status: **terms reviewed; local SDK root confirmed**.

The repository owner identified the locally installed SDK and authorized its
use for this project. The reviewed root is
`C:\Program Files\Adobe\AfterEffectsSDK`. The following expected headers were
confirmed without copying them into this repository:

- `Examples\Headers\AE_Effect.h`;
- `Examples\Headers\AEConfig.h`;
- `Examples\Headers\SP\SPBasic.h`.

The root also contains `After_Effects_SDK_Guide.pdf`. SDK builds set
`AE_SDK_ROOT` only in the build process environment.

## Terms Boundary

The current Adobe Developer Terms of Use, section 4.1, grants a limited license
to use and reproduce Developer Tools for development and testing and limits
distribution of Developer Tools or portions to approved Developer Software in
object-code form. Section 4.3 retains Adobe ownership and requires preservation
of notices. Adobe staff have additionally clarified that plug-ins built with
SDK headers/source may be distributed, while Adobe SDK headers/source may not.

Sources reviewed:

- https://wwwimages2.adobe.com/content/dam/cc/en/legal/servicetou/Developer-Terms-en_US-20240618.pdf
- https://community.adobe.com/t5/after-effects-discussions/after-effects-sdk-license/m-p/14361828#M242662

Project policy is therefore stricter than the minimum development grant:

- SDK files stay outside Git;
- SDK headers/source are used only to compile `instruments/`;
- only compiled plug-in object code may leave the local build directory, and
  no compiled `.aex` is committed here;
- SDK text or derived declarations do not cross into cleanroom `minihost/`;
- `AE_SDK_ROOT` must identify the reviewed local copy before SDK builds run.

H-2 is satisfied by the owner's identification of this installed copy and the
project's acceptance of the applicable Adobe terms and repository boundary.
This is a project compliance record, not legal advice.
