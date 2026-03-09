import Foundation
import CDivvunFST

public class SpellerArchive {
    private var archive: DFST_SpellerArchive

    public static func open(path: String) throws -> SpellerArchive {
        let exceptionCallback: cffi_exception_callback = { msg, msgLen in
        }

        let pathBytes = Array(path.utf8)
        let archive = pathBytes.withUnsafeBytes { pathPtr in
            let pathSlice = rust_slice_t(data: UnsafeMutableRawPointer(mutating: pathPtr.baseAddress), len: UInt(pathBytes.count))
            return DFST_SpellerArchive_open(pathSlice, exceptionCallback)
        }

        if archive.data == nil {
            throw NSError(domain: "DivvunFST", code: 1, userInfo: [NSLocalizedDescriptionKey: "Failed to open speller archive"])
        }

        return SpellerArchive(archive: archive)
    }

    private init(archive: DFST_SpellerArchive) {
        self.archive = archive
    }

    public func speller() throws -> Speller {
        let exceptionCallback: cffi_exception_callback = { msg, msgLen in
        }

        let speller = DFST_SpellerArchive_speller(archive, exceptionCallback)

        if speller.data == nil {
            throw NSError(domain: "DivvunFST", code: 2, userInfo: [NSLocalizedDescriptionKey: "Failed to get speller"])
        }

        return Speller(speller: speller)
    }

    public func locale() throws -> String {
        let exceptionCallback: cffi_exception_callback = { msg, msgLen in
        }

        let localeSlice = DFST_SpellerArchive_locale(archive, exceptionCallback)

        if localeSlice.data == nil {
            throw NSError(domain: "DivvunFST", code: 3, userInfo: [NSLocalizedDescriptionKey: "Failed to get locale"])
        }

        let locale = RustStr(ptr: localeSlice.data?.assumingMemoryBound(to: UInt8.self), len: Int(localeSlice.len)).toString()
        cffi_string_free(localeSlice)
        return locale
    }
}
