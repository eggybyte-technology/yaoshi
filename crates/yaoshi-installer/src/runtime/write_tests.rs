mod erase_tests {
    use super::*;

    #[test]
    fn erase_scrub_ranges_clear_head_and_tail_without_overlap() {
        let path = std::env::temp_dir().join(format!(
            "yaoshi-erase-scrub-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut file = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let target_size = TARGET_HEAD_SCRUB_BYTES + 4096 + TARGET_TAIL_SCRUB_BYTES;
        file.set_len(target_size).unwrap();
        file.write_all_at(&[0xaa], 0).unwrap();
        file.write_all_at(&[0xbb], TARGET_HEAD_SCRUB_BYTES)
            .unwrap();
        file.write_all_at(&[0xcc], target_size - 1).unwrap();

        let (head, tail) = erase_scrub_lengths(target_size);
        assert_eq!(head, TARGET_HEAD_SCRUB_BYTES);
        assert_eq!(tail, TARGET_TAIL_SCRUB_BYTES);
        write_zero_range(&mut file, 0, head).unwrap();
        write_zero_range(&mut file, target_size - tail, tail).unwrap();

        let mut byte = [0u8; 1];
        file.read_exact_at(&mut byte, 0).unwrap();
        assert_eq!(byte[0], 0);
        file.read_exact_at(&mut byte, TARGET_HEAD_SCRUB_BYTES)
            .unwrap();
        assert_eq!(byte[0], 0xbb);
        file.read_exact_at(&mut byte, target_size - 1).unwrap();
        assert_eq!(byte[0], 0);
        drop(file);
        std::fs::remove_file(path).unwrap();
    }
}
