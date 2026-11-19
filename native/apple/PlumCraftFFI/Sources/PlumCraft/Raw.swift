// The raw C calls the tests need beside PlumCore.

import PlumCraftFFI

/// `plum_close` on a core already closed: `PLUM_ERR_CORE`, never a crash.
public func plumCloseAgain(_ core: UInt64) -> Int32 { plum_close(core) }
