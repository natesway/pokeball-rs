use crate::aes::AesContext;
use crate::cert::{ChallengeData, MainChallengeData, NextChallenge};

pub fn decrypt_next_challenge<T: AesContext>(
    context: &mut T,
    data: &mut [u8; 80],
    key: &[u8; 16],
    output: &mut [u8; 16],
) -> bool {
    let (_, body, _) = unsafe { data.align_to_mut::<NextChallenge>() };
    let chal = &mut body[0];

    context.aes_set_key(key);
    aes_ctr(context, &mut chal.nonce, &chal.encrypted_challenge, output);

    let mut enc_nonce: [u8; 16] = [0; 16];
    encrypt_block(context, &chal.encrypted_hash, &chal.nonce, &mut enc_nonce);

    let mut hash_1: [u8; 16] = [0; 16];
    aes_hash(context, &chal.nonce, output, &mut hash_1);

    hash_1 == enc_nonce
}

pub fn generate_chal_0<T: AesContext>(
    context: &mut T,
    bt_mac: &[u8; 6],
    blob: &[u8; 256],
    the_challenge: &[u8; 16],
    main_nonce: &[u8; 16],
    main_key: &[u8; 16],
    outer_nonce: &[u8; 16],
    output: &mut [u8; 378],
) {
    let (_, body, _) = unsafe { output.align_to_mut::<ChallengeData>() };
    let chal = &mut body[0];

    let mut tmp_hash: [u8; 16] = [0; 16];
    let mut reversed_mac: [u8; 6] = bt_mac.clone();
    reversed_mac.reverse();

    //outer layer
    chal.blob.copy_from_slice(blob);
    chal.bt_addr.copy_from_slice(&reversed_mac);
    chal.nonce.copy_from_slice(outer_nonce);
    chal.state.copy_from_slice(&[0; 4]);

    let mut main_data = MainChallengeData::new(reversed_mac, &main_key, &main_nonce);

    context.aes_set_key(&main_key);
    aes_ctr(
        context,
        &mut main_data.nonce,
        the_challenge,
        &mut main_data.encrypted_challenge,
    );
    aes_hash(context, &mut main_data.nonce, the_challenge, &mut tmp_hash);
    encrypt_block(
        context,
        &mut tmp_hash,
        &main_data.nonce,
        &mut main_data.encrypted_hash,
    );

    unsafe {
        aes_ctr(
            context,
            &mut chal.nonce,
            any_as_u8_slice(&main_data),
            &mut tmp_hash,
        );
    }
    encrypt_block(context, &tmp_hash, &chal.nonce, &mut chal.encrypted_hash);
    unsafe {
        aes_ctr(
            context,
            &mut chal.nonce,
            any_as_u8_slice(&main_data),
            &mut chal.encrypted_main_challenge,
        );
    }
}

pub fn generate_next_chal<T: AesContext>(
    context: &mut T,
    in_data: Option<&[u8; 16]>,
    key: &[u8; 16],
    nonce: &[u8; 16],
    output: &mut [u8; 52],
) {
    let (_, body, _) = unsafe { output.align_to_mut::<NextChallenge>() };
    let chal = &mut body[0];

    let mut tmp_hash: [u8; 16] = [0; 16];

    let data = match in_data {
        Some(d) => d.clone(),
        None => [0; 16],
    };
    chal.nonce.copy_from_slice(nonce);

    context.aes_set_key(key);
    aes_ctr(
        context,
        &mut chal.nonce,
        &data,
        &mut chal.encrypted_challenge,
    );

    aes_hash(context, &mut chal.nonce, &data, &mut tmp_hash);
    encrypt_block(
        context,
        &mut tmp_hash,
        &mut chal.nonce,
        &mut chal.encrypted_challenge,
    );
}

pub fn generate_reconnect_response<T: AesContext>(
    context: &mut T,
    key: &[u8; 16],
    challenge: &[u8; 16],
    output: &mut [u8; 16],
) {
    context.aes_set_key(&key);
    context.pgp_aes_encrypt(&challenge, output);
    for i in 0..16 as usize {
        output[i] ^= challenge[i + 16];
    }
}

fn aes_ctr<T: AesContext>(context: &T, nonce: &[u8; 16], data: &[u8], output: &mut [u8]) {
    let mut ctr: [u8; 16] = [0; 16];
    let mut encrypted_ctr: [u8; 16] = [0; 16];

    init_nonce_ctr(nonce, &mut ctr);

    let blocks = data.len() / 16;
    for i in 0..blocks {
        inc_ctr(&mut ctr);
        context.pgp_aes_encrypt(&ctr, &mut encrypted_ctr);

        for j in 0..16 {
            output[16 * i + j] = encrypted_ctr[j] ^ data[16 * i + j];
        }
    }
}

fn aes_hash<T: AesContext>(context: &T, nonce: &[u8; 16], data: &[u8], output: &mut [u8; 16]) {
    let mut tmp: [u8; 16] = [0; 16];
    let mut tmp2: [u8; 16] = [0; 16];
    let mut nonce_hash: [u8; 16] = [0; 16];

    init_nonce_hash(nonce, data.len(), &mut nonce_hash);

    context.pgp_aes_encrypt(&nonce_hash, &mut tmp);

    let blocks = data.len() / 16;
    for i in 0..blocks {
        for j in 0..16 {
            tmp[j] ^= data[i * 16 + j];
        }

        tmp2.copy_from_slice(&tmp);
        context.pgp_aes_encrypt(&tmp2, &mut tmp)
    }
    output.copy_from_slice(&tmp);
}

unsafe fn any_as_u8_slice<T: Sized>(p: &T) -> &[u8] {
    core::slice::from_raw_parts((p as *const T) as *const u8, core::mem::size_of::<T>())
}

fn encrypt_block<T: AesContext>(
    context: &T,
    nonce_iv: &[u8; 16],
    nonce: &[u8; 16],
    output: &mut [u8; 16],
) {
    let mut tmp: [u8; 16] = [0; 16];
    let mut nonce_ctr: [u8; 16] = [0; 16];

    init_nonce_ctr(nonce, &mut nonce_ctr);

    context.pgp_aes_encrypt(&nonce_ctr, &mut tmp);

    for i in 0..(16 as usize) {
        output[i] = tmp[i] ^ nonce_iv[i];
    }
}

fn inc_ctr(ctr: &mut [u8; 16]) {
    if ctr[15] == u8::MAX {
        ctr[15] = 0;
        ctr[14] += 1;
    } else {
        ctr[15] += 1;
    }
}

fn init_nonce_ctr(nonce: &[u8; 16], nonce_ctr: &mut [u8; 16]) {
    nonce_ctr[1..14].copy_from_slice(&nonce[..13]);
    nonce_ctr[0] = 1;
    nonce_ctr[14] = 0;
    nonce_ctr[15] = 0;
}

fn init_nonce_hash(nonce: &[u8; 16], datalen: usize, nonce_hash: &mut [u8; 16]) {
    nonce_hash[1..14].copy_from_slice(&nonce[..13]);
    nonce_hash[0] = 57;
    nonce_hash[14] = ((datalen >> 8) & 0xff) as u8;
    nonce_hash[15] = (datalen & 0xff) as u8;
}
