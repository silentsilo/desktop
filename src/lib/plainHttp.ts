/**
 * Whether an address will be reached over plain HTTP.
 *
 * The contents stay encrypted either way, which is what makes this easy to
 * wave away. What is not encrypted is the credential used to reach the
 * storage: an S3 access key and its SigV4 signature, or a WebDAV password.
 * The object names go too, and those carry how many devices the silo has,
 * how often it is used and how large each file is.
 *
 * A warning rather than a refusal: a MinIO on the same machine, or a NAS on
 * a home LAN, is a legitimate reason to want it.
 */
export function isPlainHttp(address: string): boolean {
  return /^http:\/\//i.test(address.trim());
}

export const PLAIN_HTTP_WARNING =
  "Plain HTTP. The files stay encrypted, but the key used to reach this storage travels readable, and so does the size and timing of everything you store. Use https:// unless this is a machine on your own network.";
