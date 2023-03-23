CREATE TABLE join_sounds (
    `id` INT UNSIGNED NOT NULL AUTO_INCREMENT,
    `user` BIGINT UNSIGNED NOT NULL,
    `join_sound_id` INT UNSIGNED NOT NULL,
    `guild` BIGINT UNSIGNED,
    FOREIGN KEY (`join_sound_id`) REFERENCES sounds(id) ON DELETE CASCADE,
    PRIMARY KEY (`id`)
);

INSERT INTO join_sounds (`user`, `join_sound_id`) SELECT `user`, `join_sound_id` FROM `users` WHERE `join_sound_id` is not null;
