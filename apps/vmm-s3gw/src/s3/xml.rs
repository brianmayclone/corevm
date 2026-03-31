pub struct BucketInfo {
    pub name: String,
    pub creation_date: String,
}

pub struct ObjectInfo {
    pub key: String,
    pub last_modified: String,
    pub etag: String,
    pub size: u64,
    pub storage_class: String,
}

pub fn list_buckets_xml(buckets: &[BucketInfo], owner_id: &str) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Owner>
    <ID>"#,
    );
    xml.push_str(&xml_escape(owner_id));
    xml.push_str(
        r#"</ID>
    <DisplayName>"#,
    );
    xml.push_str(&xml_escape(owner_id));
    xml.push_str(
        r#"</DisplayName>
  </Owner>
  <Buckets>"#,
    );

    for b in buckets {
        xml.push_str("\n    <Bucket>\n      <Name>");
        xml.push_str(&xml_escape(&b.name));
        xml.push_str("</Name>\n      <CreationDate>");
        xml.push_str(&xml_escape(&b.creation_date));
        xml.push_str("</CreationDate>\n    </Bucket>");
    }

    xml.push_str(
        r#"
  </Buckets>
</ListAllMyBucketsResult>"#,
    );
    xml
}

pub fn list_objects_v2_xml(
    bucket: &str,
    prefix: &str,
    objects: &[ObjectInfo],
    is_truncated: bool,
    key_count: usize,
    max_keys: usize,
    continuation_token: &str,
    next_continuation_token: &str,
) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">"#,
    );

    xml.push_str("\n  <Name>");
    xml.push_str(&xml_escape(bucket));
    xml.push_str("</Name>");
    xml.push_str("\n  <Prefix>");
    xml.push_str(&xml_escape(prefix));
    xml.push_str("</Prefix>");
    xml.push_str(&format!("\n  <KeyCount>{}</KeyCount>", key_count));
    xml.push_str(&format!("\n  <MaxKeys>{}</MaxKeys>", max_keys));
    xml.push_str(&format!(
        "\n  <IsTruncated>{}</IsTruncated>",
        is_truncated
    ));

    if !continuation_token.is_empty() {
        xml.push_str("\n  <ContinuationToken>");
        xml.push_str(&xml_escape(continuation_token));
        xml.push_str("</ContinuationToken>");
    }
    if !next_continuation_token.is_empty() {
        xml.push_str("\n  <NextContinuationToken>");
        xml.push_str(&xml_escape(next_continuation_token));
        xml.push_str("</NextContinuationToken>");
    }

    for obj in objects {
        xml.push_str("\n  <Contents>");
        xml.push_str("\n    <Key>");
        xml.push_str(&xml_escape(&obj.key));
        xml.push_str("</Key>");
        xml.push_str("\n    <LastModified>");
        xml.push_str(&xml_escape(&obj.last_modified));
        xml.push_str("</LastModified>");
        xml.push_str("\n    <ETag>");
        xml.push_str(&xml_escape(&obj.etag));
        xml.push_str("</ETag>");
        xml.push_str(&format!("\n    <Size>{}</Size>", obj.size));
        xml.push_str("\n    <StorageClass>");
        xml.push_str(&xml_escape(&obj.storage_class));
        xml.push_str("</StorageClass>");
        xml.push_str("\n  </Contents>");
    }

    xml.push_str("\n</ListBucketResult>");
    xml
}

pub fn copy_object_result_xml(etag: &str, last_modified: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<CopyObjectResult>
  <ETag>{}</ETag>
  <LastModified>{}</LastModified>
</CopyObjectResult>"#,
        xml_escape(etag),
        xml_escape(last_modified),
    )
}

pub fn initiate_multipart_xml(bucket: &str, key: &str, upload_id: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<InitiateMultipartUploadResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Bucket>{}</Bucket>
  <Key>{}</Key>
  <UploadId>{}</UploadId>
</InitiateMultipartUploadResult>"#,
        xml_escape(bucket),
        xml_escape(key),
        xml_escape(upload_id),
    )
}

pub fn complete_multipart_xml(bucket: &str, key: &str, etag: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<CompleteMultipartUploadResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Bucket>{}</Bucket>
  <Key>{}</Key>
  <ETag>{}</ETag>
</CompleteMultipartUploadResult>"#,
        xml_escape(bucket),
        xml_escape(key),
        xml_escape(etag),
    )
}

pub fn xml_escape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&apos;"),
            _ => result.push(c),
        }
    }
    result
}
